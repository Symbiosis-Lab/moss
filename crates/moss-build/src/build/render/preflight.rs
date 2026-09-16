//! Pre-compile sanity checks: detect misplaced theme files and mixed multilingual structure.

use std::path::Path;

/// The two root-level filenames moss deliberately never reads or copies —
/// `style.css` and `script.js` shadow the canonical `.moss/theme/` location
/// and are ignored outright (warned about by [`check_misplaced_theme_files`],
/// not auto-moved). `relative_path` is vault-relative, forward-slash
/// normalized; this only matches the file sitting at the vault ROOT, not a
/// same-named file in a subdirectory.
///
/// The single authority for "is this root file outside moss's domain" —
/// every consumer that decides whether to touch a root asset (the asset
/// copy pass, the sweep's drift walk) must call this rather than
/// re-deriving the same two filenames, or the two can disagree about a file
/// neither of them will ever act on (moss#1087).
pub fn is_ignored_root_theme_file(relative_path: &str) -> bool {
    relative_path == "style.css" || relative_path == "script.js"
}

pub(crate) fn check_misplaced_theme_files(source_path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let theme_dir = source_path.join(".moss").join("theme");

    for (filename, canonical_rel) in
        [("style.css", ".moss/theme/style.css"), ("script.js", ".moss/theme/script.js")]
    {
        debug_assert!(is_ignored_root_theme_file(filename));
        let root_path = source_path.join(filename);
        let canonical_path = theme_dir.join(filename);
        if root_path.exists() && !canonical_path.exists() {
            warnings.push(format!(
                "Found {filename} at project root, but moss reads user {filename} from \
                 {canonical_rel}. Move {filename} to {canonical_rel} or it will be ignored."
            ));
        }
    }

    warnings
}

/// Detect mixed multilingual structure in the project: when a language has
/// BOTH a folder-per-language tree (e.g. `zh-hans/index.md`) AND sibling
/// suffix files (e.g. `index.zh-hans.md`) for the same language.
///
/// moss supports both styles — folder-per-language is canonical, sibling
/// suffixes are legacy but still accepted. Mixing the two in the same project
/// is ambiguous for the author ("which file wins for zh-hans?") and compounds
/// confusion across many files. This check nudges toward the canonical style
/// without forbidding either.
///
/// Algorithm: for each top-level directory that is a known language suffix
/// (e.g. `zh-hans/`, `fr/`) and contains at least one markdown file, check
/// whether any file anywhere in the project has a matching `.<lang>.md`
/// sibling suffix. If so, emit one warning per language (not per file).
///
/// Inputs:
/// * `markdown_paths` — project-relative forward-slash paths for every
///   markdown file discovered during scan (e.g. `"index.md"`,
///   `"zh-hans/index.md"`, `"posts/hello.zh.md"`).
///
/// Returns one human-readable warning string per language with mixed usage.
pub(crate) fn check_mixed_multilingual_structure(markdown_paths: &[String]) -> Vec<String> {
    use std::collections::{BTreeMap, BTreeSet};

    // For each top-level dir that is a language suffix, record the langs
    // that have folder-per-language content (at least one .md inside).
    let mut folder_langs: BTreeSet<String> = BTreeSet::new();
    // For each language suffix, record example sibling paths (one is enough
    // to cite in the warning message).
    let mut sibling_langs: BTreeMap<String, String> = BTreeMap::new();

    for path in markdown_paths {
        // Folder-per-language: the path starts with "<lang>/" and the lang
        // is a known suffix.
        if let Some(lang) = moss_core::home::lang_tree_prefix(path) {
            folder_langs.insert(lang.to_lowercase());
            // SCOPE: files inside a language-folder tree are NOT considered
            // sibling-suffix candidates even if they happen to match (e.g.
            // `zh-hans/foo.en.md`). We only flag sibling files at project
            // root, matching the top-level scope of `lang_tree_prefix`.
            // Keeping the scope symmetric matches moss's documented
            // "top-level folder-per-language" canonical form.
            continue;
        }

        // Sibling suffix: the file stem ends in ".<lang>" (e.g. "index.zh-hans").

        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !stem.is_empty() && moss_core::home::strip_lang_suffix(stem).is_some() {
            // strip_lang_suffix returned Some, so the stem has a `.<suffix>`
            // where `<suffix>` is a known lang code. Extract the suffix via
            // rsplit_once for safer handling than arithmetic on the stem.
            if let Some((_, suf)) = stem.rsplit_once('.') {
                let lang_suffix = suf.to_lowercase();
                sibling_langs
                    .entry(lang_suffix)
                    .or_insert_with(|| path.clone());
            }
        }
    }

    // For each lang with BOTH folder-per-language AND a sibling sample,
    // emit one warning.
    let mut warnings = Vec::new();
    for (lang, example_path) in &sibling_langs {
        if folder_langs.contains(lang) {
            warnings.push(format!(
                "Mixed multilingual structure detected for language '{lang}'. \
                 Found both sibling suffix file ({example_path}) and folder-based \
                 translation ({lang}/...). Prefer folder-per-language — see \
                 https://docs.mosspub.com/multilingual for the canonical pattern."
            ));
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::{check_misplaced_theme_files, check_mixed_multilingual_structure};
    use std::fs;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_warns_when_mixed_multilingual_structure() {
        // Folder-per-language (zh-hans/index.md) AND sibling suffix
        // (index.zh-hans.md) both present for the same language.
        let md_files = paths(&[
            "index.md",
            "index.zh-hans.md",
            "zh-hans/index.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(
            warnings.iter().any(|w| w.contains("Mixed multilingual") && w.contains("zh-hans")),
            "Expected mixed-structure warning for zh-hans. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_no_warning_for_folder_only_multilingual() {
        let md_files = paths(&[
            "index.md",
            "zh-hans/index.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(
            warnings.is_empty(),
            "Folder-only should not warn. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_no_warning_for_sibling_only_multilingual() {
        // Legacy sibling-suffix style without any folder-per-language tree.
        // We don't forbid it — just discourage mixing.
        let md_files = paths(&[
            "index.md",
            "index.zh-hans.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(
            warnings.is_empty(),
            "Sibling-only should not warn. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_no_warning_for_monolingual_project() {
        let md_files = paths(&["index.md", "about.md", "posts/hello.md"]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(warnings.is_empty(), "Monolingual should not warn. Got: {:?}", warnings);
    }

    #[test]
    fn test_warns_once_per_mixed_language() {
        // Multiple sibling files in zh-hans shouldn't produce multiple warnings.
        let md_files = paths(&[
            "index.md",
            "index.zh-hans.md",
            "about.zh-hans.md",
            "posts/hello.zh-hans.md",
            "zh-hans/index.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        let zh_warnings: Vec<&String> = warnings
            .iter()
            .filter(|w| w.contains("zh-hans"))
            .collect();
        assert_eq!(zh_warnings.len(), 1, "Expected exactly one warning for zh-hans. Got: {:?}", warnings);
    }

    #[test]
    fn test_warns_per_language_when_multiple_mixed() {
        // Both fr and zh-hans are mixed — one warning each.
        let md_files = paths(&[
            "index.md",
            "index.zh-hans.md",
            "zh-hans/index.md",
            "index.fr.md",
            "fr/index.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(warnings.iter().any(|w| w.contains("zh-hans")));
        assert!(warnings.iter().any(|w| w.contains("'fr'")));
        assert_eq!(warnings.len(), 2, "Expected one warning per mixed language. Got: {:?}", warnings);
    }

    #[test]
    fn test_no_warning_when_different_langs_use_different_styles() {
        // zh-hans uses folder-per-language; fr uses sibling suffix. No overlap
        // within a single language, so no warning — authors who stick to one
        // style per language aren't mixing.
        let md_files = paths(&[
            "index.md",
            "zh-hans/index.md",
            "index.fr.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(
            warnings.is_empty(),
            "Different langs using different styles shouldn't warn. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_ignores_sibling_suffix_inside_language_folder() {
        // A file at zh-hans/foo.en.md is NOT a conflict — the file happens to
        // have an .en.md suffix but it's already inside a language-folder
        // tree. Don't warn. This keeps sibling detection symmetric with
        // lang_tree_prefix (both are top-level scope only).
        let md_files = paths(&[
            "index.md",
            "zh-hans/index.md",
            // Accidental/stray file inside language tree; not a conflict.
            "zh-hans/foo.en.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        assert!(
            warnings.is_empty(),
            "Sibling inside language folder should not warn. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_subdirectory_mixed_structure_not_currently_detected() {
        // DOCUMENTED LIMITATION: mixing folder-per-language and sibling suffix
        // in a subdirectory (not project root) is NOT currently warned.
        // Example: docs/zh-hans/foo.md + docs/foo.zh-hans.md.
        //
        // If this becomes a real problem, extend lang_tree_prefix to match
        // any path component (not just the first) and update the sibling
        // scope accordingly. See review notes on Task 2.11.
        let md_files = paths(&[
            "docs/foo.md",
            // Sibling style in subdirectory.
            "docs/foo.zh-hans.md",
            // Folder style in subdirectory.
            "docs/zh-hans/foo.md",
        ]);
        let warnings = check_mixed_multilingual_structure(&md_files);
        // Currently: no warning (subdirectory scope not detected).
        // If this test fails because someone extended detection, that's GOOD
        // — update the assertion to expect the warning.
        assert!(
            warnings.is_empty(),
            "Subdirectory mixing is currently NOT detected. Test pins the limitation. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_warns_when_style_css_at_project_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("style.css"), "body { color: red; }").unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert!(
            warnings.iter().any(|w| w.contains("style.css") && w.contains(".moss/theme")),
            "Expected warning about style.css canonical location. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_warns_when_script_js_at_project_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("script.js"), "console.log('hi');").unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert!(
            warnings.iter().any(|w| w.contains("script.js") && w.contains(".moss/theme")),
            "Expected warning about script.js canonical location. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_no_warning_when_style_css_at_canonical_location() {
        let temp = tempfile::tempdir().unwrap();
        let theme_dir = temp.path().join(".moss").join("theme");
        fs::create_dir_all(&theme_dir).unwrap();
        fs::write(theme_dir.join("style.css"), "body { color: red; }").unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert!(
            warnings.is_empty(),
            "Expected no warnings when style.css is at canonical location. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_no_warning_when_no_theme_files_exist() {
        let temp = tempfile::tempdir().unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert!(warnings.is_empty(), "Expected no warnings for empty project. Got: {:?}", warnings);
    }

    #[test]
    fn test_no_warning_when_canonical_exists_even_if_root_exists() {
        // If user has BOTH canonical and root files, moss uses the canonical
        // file. No warning — the user is effectively using the right location
        // and the root file is just noise we don't need to scold about.
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("style.css"), "body { color: red; }").unwrap();
        let theme_dir = temp.path().join(".moss").join("theme");
        fs::create_dir_all(&theme_dir).unwrap();
        fs::write(theme_dir.join("style.css"), "body { color: blue; }").unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert!(
            warnings.iter().all(|w| !w.contains("style.css")),
            "Expected no style.css warning when canonical file exists. Got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_warns_for_both_style_css_and_script_js_at_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("style.css"), "body {}").unwrap();
        fs::write(temp.path().join("script.js"), "var x;").unwrap();

        let warnings = check_misplaced_theme_files(temp.path());

        assert_eq!(warnings.len(), 2, "Expected 2 warnings. Got: {:?}", warnings);
        assert!(warnings.iter().any(|w| w.contains("style.css")));
        assert!(warnings.iter().any(|w| w.contains("script.js")));
    }

}
