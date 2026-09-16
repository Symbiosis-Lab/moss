//! Home file detection for the moss content model.
//!
//! Defines which filenames are recognized as a folder's "home" (index) page.
//! This is part of moss's content model — all generators and SSG plugins use
//! these conventions, ensuring consistent folder structure regardless of which
//! SSG backend is active.
//!
//! # Priority Order
//!
//! When multiple home file candidates exist in the same folder, the first
//! match in `INDEX_STEMS` wins:
//!
//! 1. `index` — standard convention
//! 2. `readme` — Git/GitHub convention
//! 3. `_index` — Hugo convention
//! 4. `main` — general fallback
//!
//! All matching is case-insensitive: `README.md`, `Index.md`, `MAIN.md` all match.

/// Recognized file stems (case-insensitive) that act as a folder's index page.
/// Priority order: first match wins when multiple candidates exist.
///
/// This is the single source of truth for home file detection across the entire
/// moss codebase. The Tauri layer, content graph, and SSG plugins all derive
/// their behavior from this constant.
pub const INDEX_STEMS: &[&str] = &["index", "readme", "_index", "main"];

/// Known language codes — the SINGLE list governing every "is this a language?"
/// decision in moss: which top-level folder is a language tree
/// ([`lang_tree_prefix`], used by the build's `has_language_trees` AND the email
/// audience picker), which filename suffix is a language suffix
/// ([`strip_lang_suffix`]), and which folder/file is a home page.
///
/// This is moss's file-system-driven judge of a site's languages: a top-level
/// folder named one of these codes IS that language. Extend this list (and only
/// this list) to recognize a new language tree — there is no second allowlist.
///
/// This is a hardcoded list used only in moss-core (which has no access to the
/// i18n module). Bare `"zh"` is accepted as shorthand for Simplified Chinese
/// (`zh-hans`); the normalization from `zh` → `zh-hans` happens in the i18n
/// layer (`Language::from_code`), not here. This list only governs which codes
/// are recognized as languages at all.
const KNOWN_LANGUAGES: &[(&str, &str)] = &[
    // (code, endonym). The endonym is what a language switcher shows for a
    // language moss has no interface for: a reader who wants the German
    // edition is looking for "Deutsch", not "DE". The three moss DOES
    // translate its interface into keep their own short chips (`i18n`'s
    // `Language::display_name`), so this column is only consulted for the rest.
    //
    // One table, not a list plus a lookup: a code with no endonym would render
    // a blank chip, and the two could only drift apart.

    // ISO-639-1 two-letter codes (most common languages)
    ("en", "English"), ("zh", "中文"), ("ja", "日本語"), ("ko", "한국어"),
    ("de", "Deutsch"), ("fr", "Français"), ("es", "Español"), ("it", "Italiano"),
    ("pt", "Português"), ("ru", "Русский"), ("ar", "العربية"), ("hi", "हिन्दी"),
    ("tr", "Türkçe"), ("pl", "Polski"), ("nl", "Nederlands"), ("sv", "Svenska"),
    ("da", "Dansk"), ("fi", "Suomi"), ("no", "Norsk"), ("cs", "Čeština"),
    ("hu", "Magyar"), ("ro", "Română"), ("el", "Ελληνικά"), ("vi", "Tiếng Việt"),
    ("th", "ไทย"), ("id", "Bahasa Indonesia"), ("he", "עברית"), ("uk", "Українська"),
    ("bg", "Български"), ("hr", "Hrvatski"), ("sr", "Српски"), ("sk", "Slovenčina"),
    ("sl", "Slovenščina"), ("et", "Eesti"), ("lv", "Latviešu"), ("lt", "Lietuvių"),
    // Less common but real-use language codes (Welsh, Maori, Tibetan, Scots
    // Gaelic, Basque, Catalan, Galician, Swahili) — keeps the long-tail
    // surprise rate low without expanding to all of ISO-639.
    ("cy", "Cymraeg"), ("mi", "Māori"), ("bo", "བོད་སྐད་"), ("gd", "Gàidhlig"),
    ("eu", "Euskara"), ("ca", "Català"), ("gl", "Galego"), ("sw", "Kiswahili"),
    // Region/script-tagged variants seen in practice.
    ("zh-hans", "简体中文"), ("zh-hant", "繁體中文"), ("zh-cn", "简体中文"),
    ("zh-tw", "繁體中文"), ("pt-br", "Português (Brasil)"),
    ("en-us", "English (US)"), ("en-gb", "English (UK)"),
];


/// Every language code moss recognizes as a language tree or filename suffix.
///
/// Exposed so `moss describe --json` can publish the allowlist. An agent asked
/// to add an edition has to pick the directory name *before* it can build and
/// see whether the name worked, and an unrecognized name fails silently — the
/// tree is treated as ordinary content, so there is no switcher, no `<html
/// lang>`, and no error. Inventing `english/` is the documented failure. This
/// list is the only allowlist, so publishing it is what makes the guidance
/// ("use one of these") checkable rather than a claim the agent has to trust.
pub fn known_language_codes() -> Vec<&'static str> {
    KNOWN_LANGUAGES.iter().map(|(code, _)| *code).collect()
}

/// If `path` is rooted under a known language-tree directory (e.g.
/// `"zh-hans/about.md"`), return that directory component (e.g. `"zh-hans"`).
///
/// Returns `None` when the path has no parent directory (root-level file)
/// or when the first path component is not a recognized language code.
/// Matching is case-insensitive.
///
/// Used by wikilink resolution to prefer same-language-tree candidates.
///
/// ```
/// use moss_core::home::lang_tree_prefix;
/// assert_eq!(lang_tree_prefix("zh-hans/about.md"), Some("zh-hans"));
/// assert_eq!(lang_tree_prefix("ZH-HANS/about.md"), Some("ZH-HANS"));
/// assert_eq!(lang_tree_prefix("fr/deep/page.md"), Some("fr"));
/// assert_eq!(lang_tree_prefix("posts/hello.md"), None);
/// assert_eq!(lang_tree_prefix("index.md"), None);
/// assert_eq!(lang_tree_prefix(""), None);
/// ```
pub fn lang_tree_prefix(path: &str) -> Option<&str> {
    let first = path.split('/').next()?;
    // Must have at least one more component after it (so it's a directory).
    if first.len() == path.len() {
        return None;
    }
    if is_known_language_code(first) {
        Some(first)
    } else {
        None
    }
}

/// Strip a known language suffix from a file stem.
///
/// Returns the bare stem if the suffix after the last `.` is a recognized
/// language code, otherwise returns `None`.
///
/// ```
/// assert_eq!(moss_core::home::strip_lang_suffix("index.zh-hans"), Some("index"));
/// assert_eq!(moss_core::home::strip_lang_suffix("index.v2"), None);
/// assert_eq!(moss_core::home::strip_lang_suffix("index"), None);
/// ```
pub fn strip_lang_suffix(stem: &str) -> Option<&str> {
    let (head, suffix) = stem.rsplit_once('.')?;
    if is_known_language_code(suffix) {
        Some(head)
    } else {
        None
    }
}

/// The language suffix itself — the other half of [`strip_lang_suffix`].
///
/// ```
/// assert_eq!(moss_core::home::lang_suffix("index.ja"), Some("ja"));
/// assert_eq!(moss_core::home::lang_suffix("index.v2"), None);
/// ```
pub fn lang_suffix(stem: &str) -> Option<&str> {
    let (_, suffix) = stem.rsplit_once('.')?;
    if is_known_language_code(suffix) {
        Some(suffix)
    } else {
        None
    }
}

/// Whether `code` is a language moss recognizes at all.
///
/// "Recognizes" is a wider question than "has an interface for". moss ships UI
/// strings in three languages; it recognizes ~50 as tree names and filename
/// suffixes. A `ja/` tree is real — wikilinks resolve within it,
/// [`strip_lang_suffix`] honors it, [`lang_tree_prefix`] returns it — it simply
/// gets the default edition's chrome, because there are no Japanese strings to
/// render it with. Keeping the two questions apart is what stopped a Japanese
/// page being served as `lang="zh-Hant"` (#977).
///
/// ```
/// assert!(moss_core::home::is_known_language_code("ja"));
/// assert!(moss_core::home::is_known_language_code("ZH-Hant"));
/// assert!(!moss_core::home::is_known_language_code("english"));
/// ```
pub fn is_known_language_code(code: &str) -> bool {
    let code = code.to_lowercase();
    KNOWN_LANGUAGES.iter().any(|(c, _)| *c == code)
}

/// The language's name in ITSELF — `Deutsch`, not `German` and not `DE`.
///
/// `None` only for a code outside the allowlist, which the callers on the
/// switcher path have already rejected. Case-insensitive, because a tag
/// arrives here canonicalized (`en-GB`) while the table is keyed lowercase.
pub fn endonym(code: &str) -> Option<&'static str> {
    let code = code.to_lowercase();
    KNOWN_LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, name)| *name)
}

/// Check if a filename stem (without extension) is a recognized home file.
///
/// Matching is case-insensitive.
///
/// ```
/// assert!(moss_core::home::is_index_stem("index"));
/// assert!(moss_core::home::is_index_stem("README"));
/// assert!(!moss_core::home::is_index_stem("about"));
/// ```
pub fn is_index_stem(stem: &str) -> bool {
    INDEX_STEMS.contains(&stem.to_lowercase().as_str())
}

/// Check if a file stem acts as a home/index file in a given folder context.
///
/// Returns true if the stem is a recognized index stem (index, readme, etc.),
/// including language-suffixed variants like `index.zh-hans`.
/// Also matches self-named folder notes. Matching is case-insensitive.
pub fn is_home_file(stem: &str, parent_folder_name: &str) -> bool {
    // Direct index stem match
    if is_index_stem(stem) {
        return true;
    }

    // Language-suffixed index stem (e.g., "index.zh-hans")
    if let Some(bare) = strip_lang_suffix(stem) {
        if is_index_stem(bare) {
            return true;
        }
    }

    // Self-named folder note
    !parent_folder_name.is_empty() && stem.to_lowercase() == parent_folder_name.to_lowercase()
}

/// Find the home file among filenames, with folder-name awareness.
///
/// Like [`detect_home_file`], but also recognizes self-named folder notes
/// (e.g., `recipes.md` inside a folder named `recipes`). Priority order:
///
/// 1. INDEX_STEMS × .md (index.md > readme.md > _index.md > main.md)
/// 2. Language-suffixed index stems × .md (index.zh-hans.md > readme.en.md > ...)
/// 3. index.{pages,docx}
/// 4. Self-named: `foldername.md` (where foldername matches parent folder)
/// 5. First document alphabetically
pub fn detect_home_file_in_folder<'a>(
    filenames: &[&'a str],
    folder_name: &str,
) -> Option<&'a str> {
    // Priority 1: bare index stems × .md
    for stem in INDEX_STEMS {
        let target_md = format!("{}.md", stem);
        if let Some(&f) = filenames.iter().find(|f| f.to_lowercase() == target_md) {
            return Some(f);
        }
    }

    // Priority 2: language-suffixed index stems × .md
    // e.g., index.zh-hans.md, readme.en.md
    for stem in INDEX_STEMS {
        if let Some(&f) = filenames.iter().find(|f| {
            let lower = f.to_lowercase();
            if let Some(name_without_ext) = lower.strip_suffix(".md") {
                if let Some(bare) = strip_lang_suffix(name_without_ext) {
                    return bare == *stem;
                }
            }
            false
        }) {
            return Some(f);
        }
    }

    // Priority 3: index × non-markdown
    for ext in &["pages", "docx"] {
        let target = format!("index.{}", ext);
        if let Some(&f) = filenames.iter().find(|f| f.to_lowercase() == target) {
            return Some(f);
        }
    }

    // Priority 4: self-named folder note
    let self_named = format!("{}.md", folder_name.to_lowercase());
    if let Some(&f) = filenames.iter().find(|f| f.to_lowercase() == self_named) {
        return Some(f);
    }

    // Priority 5: first document alphabetically
    let mut doc_files: Vec<&&str> = filenames
        .iter()
        .filter(|f| {
            let lower = f.to_lowercase();
            lower.ends_with(".md")
                || lower.ends_with(".pages")
                || lower.ends_with(".docx")
                || lower.ends_with(".doc")
        })
        .collect();
    doc_files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    doc_files.first().map(|f| **f)
}

/// Marker-aware variant of [`detect_home_file_in_folder`].
///
/// A file whose frontmatter carries the `home: true` marker wins over every
/// filename-based rule, mirroring the renderer's *marker-based* election order
/// (marker › index › self-named › fallback). This keeps plugin-facing
/// detection in agreement with the built site even across folder renames: a
/// marked file stays the home when its name no longer matches the folder.
///
/// Note: this does NOT replicate the renderer's translation-inheritance
/// promotion (a marker-less translation of a marked home inheriting home-ness);
/// only explicit `home: true` markers are honored here.
///
/// `marked` holds the basenames (matched case-insensitively) in this folder
/// whose frontmatter set `home: true`. When several are marked the
/// alphabetically-first wins (deterministic; the build separately warns on
/// multi-home collisions). With no marked files this is identical to
/// [`detect_home_file_in_folder`].
pub fn detect_home_file_in_folder_marked<'a>(
    filenames: &[&'a str],
    folder_name: &str,
    marked: &[&str],
) -> Option<&'a str> {
    if !marked.is_empty() {
        let mut hits: Vec<&'a str> = filenames
            .iter()
            .copied()
            .filter(|f| marked.iter().any(|m| m.eq_ignore_ascii_case(f)))
            .collect();
        hits.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        if let Some(&f) = hits.first() {
            return Some(f);
        }
    }
    detect_home_file_in_folder(filenames, folder_name)
}

/// Resolve the site name from the home page, structurally.
///
/// This is the single decision that both the static `<title>` path and the
/// bundled-SPA `og:title` path route through, so the two never disagree (#775).
///
/// Inputs:
/// - `homepage_filename`: the home file's basename (e.g. `"index.md"`,
///   `"刘果.md"`), or `None` when the project has no home page.
/// - `homepage_title`: the home page's title candidate — either the genuine
///   frontmatter `title:` (callers that read frontmatter directly, e.g.
///   `resolve_site_name`) or the pipeline's already-resolved `doc.title` (the
///   render sites). For an index home with no `title:`, the resolved candidate
///   is the folder name (post root-aware fix #775); for a genuine `title:` it
///   is that title. `None`/empty when unavailable.
/// - `folder_name`: the project's root folder basename — the structural
///   fallback.
///
/// Decision:
/// 1. No candidate title → `folder_name` (normalized via
///    [`crate::heading::filename_text`] for hyphen/underscore → space).
/// 2. The home file is an index-stem / self-named folder note AND the candidate
///    is a bare stem fallback (equals the folder-name fallback, or is itself a
///    raw index stem like "index"/"readme") → `folder_name`. This is the
///    STRUCTURAL guard: it keys off the FILENAME being an index home, so a page
///    GENUINELY titled "Index" in a non-index file is NOT suppressed (the old
///    value-string `is_index_stem(title)` heuristic mis-fired on that).
/// 3. Otherwise → the candidate title (a genuine user-chosen title, including a
///    `title:` set on an index home).
pub fn site_name(
    homepage_filename: Option<&str>,
    homepage_title: Option<&str>,
    folder_name: &str,
) -> String {
    let folder_label = crate::heading::filename_text(folder_name);
    let folder_label = if folder_label.is_empty() {
        folder_name.to_string()
    } else {
        folder_label
    };

    let title = match homepage_title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t,
        None => return folder_label, // (1)
    };

    let is_index_home = homepage_filename
        .map(|f| {
            let stem = std::path::Path::new(f)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(f);
            is_home_file(stem, folder_name)
        })
        .unwrap_or(false);

    // (2) Structural guard: only on an index/self-named home, and only when the
    // candidate is a filename fallback (the resolved folder-name fallback, or a
    // bare index stem that leaked from a non-root-aware path). A genuine title:
    // on an index home (candidate != fallback, not a raw stem) flows to (3).
    if is_index_home && (title == folder_label || is_index_stem(title)) {
        return folder_label;
    }

    title.to_string() // (3)
}

/// Find the home file among a list of filenames in a single folder.
///
/// Convenience wrapper around [`detect_home_file_in_folder`] without
/// folder-name context. Self-named folder notes won't be recognized;
/// use `detect_home_file_in_folder` when the folder name is available.
pub fn detect_home_file<'a>(filenames: &[&'a str]) -> Option<&'a str> {
    detect_home_file_in_folder(filenames, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_index_stem ---

    #[test]
    fn test_is_index_stem_all_recognized() {
        assert!(is_index_stem("index"));
        assert!(is_index_stem("readme"));
        assert!(is_index_stem("_index"));
        assert!(is_index_stem("main"));
    }

    #[test]
    fn test_is_index_stem_case_insensitive() {
        assert!(is_index_stem("INDEX"));
        assert!(is_index_stem("README"));
        assert!(is_index_stem("Readme"));
        assert!(is_index_stem("MAIN"));
        assert!(is_index_stem("_Index"));
    }

    #[test]
    fn test_is_index_stem_rejects_non_stems() {
        assert!(!is_index_stem("about"));
        assert!(!is_index_stem("home"));
        assert!(!is_index_stem(""));
    }

    // --- detect_home_file ---

    #[test]
    fn test_detect_home_index_md_wins() {
        let files = vec!["README.md", "index.md", "main.md"];
        assert_eq!(detect_home_file(&files), Some("index.md"));
    }

    #[test]
    fn test_detect_home_readme_over_underscore_index() {
        let files = vec!["_index.md", "README.md"];
        assert_eq!(detect_home_file(&files), Some("README.md"));
    }

    #[test]
    fn test_detect_home_underscore_index_over_main() {
        let files = vec!["main.md", "_index.md"];
        assert_eq!(detect_home_file(&files), Some("_index.md"));
    }

    #[test]
    fn test_detect_home_case_insensitive() {
        let files = vec!["INDEX.MD"];
        // Should match even with uppercase
        assert_eq!(detect_home_file(&files), Some("INDEX.MD"));
    }

    #[test]
    fn test_detect_home_index_pages() {
        let files = vec!["index.pages"];
        assert_eq!(detect_home_file(&files), Some("index.pages"));
    }

    #[test]
    fn test_detect_home_md_over_pages() {
        let files = vec!["index.pages", "readme.md"];
        // readme.md (stem priority 2, .md) beats index.pages (non-md fallback)
        assert_eq!(detect_home_file(&files), Some("readme.md"));
    }

    #[test]
    fn test_detect_home_no_candidates() {
        let files = vec!["photo.jpg", "style.css"];
        assert_eq!(detect_home_file(&files), None);
    }

    #[test]
    fn test_detect_home_fallback_to_first_doc_alphabetically() {
        let files = vec!["zebra.md", "about.md"];
        assert_eq!(detect_home_file(&files), Some("about.md"));
    }

    #[test]
    fn test_detect_home_empty_list() {
        let files: Vec<&str> = vec![];
        assert_eq!(detect_home_file(&files), None);
    }

    // --- detect_home_file_in_folder ---

    #[test]
    fn test_detect_self_named_folder_note() {
        let files = vec!["recipes.md", "pasta.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "recipes"),
            Some("recipes.md")
        );
    }

    #[test]
    fn test_index_beats_self_named() {
        let files = vec!["recipes.md", "index.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "recipes"),
            Some("index.md")
        );
    }

    #[test]
    fn test_self_named_case_insensitive() {
        let files = vec!["Recipes.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "recipes"),
            Some("Recipes.md")
        );
    }

    #[test]
    fn test_readme_beats_self_named() {
        let files = vec!["recipes.md", "readme.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "recipes"),
            Some("readme.md")
        );
    }

    #[test]
    fn test_self_named_beats_alphabetical_fallback() {
        // Without folder context, "about.md" wins alphabetically.
        // With folder context, "recipes.md" wins as self-named.
        let files = vec!["about.md", "recipes.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "recipes"),
            Some("recipes.md")
        );
    }

    // --- detect_home_file_in_folder_marked (home: true marker) ---

    #[test]
    fn test_marker_beats_index() {
        // marker › index › self-named: a `home: true` file wins over index.md.
        assert_eq!(
            detect_home_file_in_folder_marked(
                &["index.md", "home.md", "a.md"],
                "anyfolder",
                &["home.md"],
            ),
            Some("home.md")
        );
    }

    #[test]
    fn test_marker_survives_folder_rename() {
        // Folder renamed to "newname": "oldname.md" is no longer self-named, but
        // its `home: true` marker keeps it the home (the rename robustness case).
        assert_eq!(
            detect_home_file_in_folder_marked(&["oldname.md", "b.md"], "newname", &["oldname.md"]),
            Some("oldname.md")
        );
    }

    #[test]
    fn test_no_marker_matches_filename_detection() {
        // Empty marker set → identical to the filename-only detector.
        let files = ["index.md", "z.md"];
        assert_eq!(
            detect_home_file_in_folder_marked(&files, "f", &[]),
            detect_home_file_in_folder(&files, "f"),
        );
    }

    #[test]
    fn test_multiple_markers_pick_alphabetically_first() {
        // Deterministic when several files are marked (build warns separately).
        assert_eq!(
            detect_home_file_in_folder_marked(&["b.md", "a.md"], "f", &["b.md", "a.md"]),
            Some("a.md")
        );
    }

    // --- is_home_file ---

    #[test]
    fn test_is_home_file_index_stem() {
        assert!(is_home_file("index", "anything"));
        assert!(is_home_file("readme", "anything"));
    }

    #[test]
    fn test_is_home_file_self_named() {
        assert!(is_home_file("recipes", "recipes"));
        assert!(is_home_file("刘果", "刘果"));
    }

    #[test]
    fn test_is_home_file_self_named_case_insensitive() {
        assert!(is_home_file("Recipes", "recipes"));
        assert!(is_home_file("recipes", "Recipes"));
    }

    #[test]
    fn test_is_home_file_no_match() {
        assert!(!is_home_file("about", "recipes"));
        assert!(!is_home_file("about", ""));
    }

    #[test]
    fn test_is_home_file_empty_folder_name() {
        // Empty folder name should not match anything except index stems
        assert!(is_home_file("index", ""));
        assert!(!is_home_file("about", ""));
    }

    #[test]
    fn test_in_folder_delegates_to_detect_home_file_for_index_stems() {
        // When index stems exist, folder name doesn't matter
        let files = vec!["index.md", "other.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "myfolder"),
            Some("index.md")
        );
    }

    // --- i18n: language-suffixed home files ---

    #[test]
    fn test_is_home_file_with_zh_hans_suffix() {
        assert!(is_home_file("index.zh-hans", "anything"));
    }

    #[test]
    fn test_is_home_file_with_en_suffix() {
        assert!(is_home_file("index.en", "anything"));
    }

    #[test]
    fn test_is_home_file_with_zh_hant_suffix() {
        assert!(is_home_file("readme.zh-hant", "anything"));
    }

    #[test]
    fn test_is_home_file_non_language_suffix_rejected() {
        // "v2" is not a language code — should NOT be treated as home file
        assert!(!is_home_file("index.v2", "anything"));
    }

    #[test]
    fn test_detect_home_bare_stem_wins_over_lang_suffix() {
        // When both bare index.md and index.zh-hans.md exist, bare wins
        let files = vec!["index.md", "index.zh-hans.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "root"),
            Some("index.md")
        );
    }

    #[test]
    fn test_detect_home_lang_suffix_recognized_when_no_bare() {
        // When only a language-suffixed index exists, it should still be home
        let files = vec!["index.zh-hans.md", "about.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "root"),
            Some("index.zh-hans.md")
        );
    }

    // --- Task 2.6: `zh` as shorthand for `zh-hans` ---

    #[test]
    fn test_strip_lang_suffix_zh_shorthand_accepted() {
        // `.zh` should be recognized as a language suffix (shorthand for zh-hans)
        assert_eq!(strip_lang_suffix("about.zh"), Some("about"));
    }

    #[test]
    fn test_strip_lang_suffix_zh_hans_still_works() {
        // Backward compat: zh-hans still works
        assert_eq!(strip_lang_suffix("about.zh-hans"), Some("about"));
    }

    #[test]
    fn test_strip_lang_suffix_zh_hant_still_works() {
        // zh-hant stays distinct (strip_lang_suffix only returns stem)
        assert_eq!(strip_lang_suffix("about.zh-hant"), Some("about"));
    }

    #[test]
    fn test_strip_lang_suffix_zh_tw_still_works() {
        // zh-tw stays distinct (strip_lang_suffix only returns stem)
        assert_eq!(strip_lang_suffix("about.zh-tw"), Some("about"));
    }

    #[test]
    fn test_is_home_file_with_zh_shorthand_suffix() {
        // `index.zh.md` stem is `index.zh` — should be recognized as home
        assert!(is_home_file("index.zh", "anything"));
    }

    #[test]
    fn test_detect_home_zh_shorthand_recognized() {
        // index.zh.md (shorthand) should be recognized as home when no bare exists
        let files = vec!["index.zh.md", "about.md"];
        assert_eq!(
            detect_home_file_in_folder(&files, "root"),
            Some("index.zh.md")
        );
    }

    // --- site_name (#775): one structural decision for <title> + og:title ---

    #[test]
    fn test_site_name_root_index_no_title_uses_folder() {
        // The bug: a root index.md with no frontmatter title. Post root-aware
        // fix the title is already "My Site", but even if a stem leaked in,
        // the structural index-stem check resolves to the folder name.
        assert_eq!(
            site_name(Some("index.md"), Some("My Site"), "My Site"),
            "My Site"
        );
        assert_eq!(site_name(Some("index.md"), Some("index"), "My Site"), "My Site");
        assert_eq!(site_name(Some("index.md"), None, "My Site"), "My Site");
    }

    #[test]
    fn test_site_name_frontmatter_title_on_index_home_wins() {
        // A genuine `title: My Blog` on a root `index.md` must be the site name
        // — the structural index-stem guard only fires for stem FALLBACKS, not
        // real titles.
        assert_eq!(
            site_name(Some("index.md"), Some("My Blog"), "site-folder"),
            "My Blog"
        );
    }

    #[test]
    fn test_site_name_titled_index_not_suppressed() {
        // A page GENUINELY titled "Index" living in a non-index file must keep
        // its title — the old value-string heuristic wrongly suppressed this.
        assert_eq!(
            site_name(Some("about.md"), Some("Index"), "My Site"),
            "Index"
        );
    }

    #[test]
    fn test_site_name_frontmatter_title_wins_for_article_home() {
        // A self-named or non-index home file with a real title keeps it.
        assert_eq!(
            site_name(Some("about.md"), Some("My Blog"), "folder"),
            "My Blog"
        );
    }

    #[test]
    fn test_site_name_self_named_home_uses_folder() {
        // `recipes/recipes.md` style self-named home — folder name wins.
        assert_eq!(
            site_name(Some("recipes.md"), Some("recipes"), "recipes"),
            "recipes"
        );
    }

    #[test]
    fn test_site_name_no_homepage_falls_back_to_folder() {
        assert_eq!(site_name(None, None, "My Site"), "My Site");
    }

    #[test]
    fn test_site_name_folder_name_normalized() {
        // Folder name hyphens/underscores normalize to spaces.
        assert_eq!(site_name(Some("index.md"), None, "my-site"), "my site");
    }

    #[test]
    fn test_site_name_readme_home_uses_folder() {
        assert_eq!(
            site_name(Some("README.md"), Some("README"), "Docs"),
            "Docs"
        );
    }
}
