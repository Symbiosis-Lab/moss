//! Per-folder inferred language, computed once per build in the
//! scan/reduce phase and fed into `crate::i18n::resolve_document_language`
//! (via `process_markdown_file`'s `folder_lang` parameter) as a richer
//! source for rung 3 — the same slot [`crate::i18n::path::ancestor_lang_from_path`]
//! already fills when a folder is literally NAMED after a language.
//!
//! ## Why this exists
//!
//! moss used to call `i18n::detect::detect_language` on every page's own
//! body, on every build. That made a document's language move on a typo
//! fix, which fed a fingerprint and could force a full site render. The
//! fix is not to delete content detection — a vault with no
//! naming convention still needs SOME answer better than the site default
//! — it's to run it once per FOLDER, over the folder's whole file set,
//! instead of once per PAGE, over one file's bytes.
//!
//! ## The stability invariant
//!
//! Editing one file's body must never change what this module returns for
//! its folder — only adding or removing a file in that folder may, or the
//! author DECLARING the folder's language in its index (see
//! [`declared_folder_lang`], which is read on every build precisely because a
//! declaration is not a body and must not wait for a membership change). That
//! is enforced structurally, not by chance: [`resolve_folder_languages`]
//! persists each folder's last-inferred language ALONGSIDE the sorted list
//! of file paths it was inferred from ([`FolderLangCache`], mirroring
//! [`super::frontmatter_cache::FrontmatterScanCache`]'s persistence
//! pattern). A rebuild that finds the same file set for a folder reuses the
//! stored language without reading any body at all; a body is read again only
//! when the SET changed. (A declaration is not a body and must not wait for a
//! membership change, so it is consulted every build — but it comes from the
//! frontmatter-scan cache the page-map pass has already filled, not from a
//! fresh read per folder.)

use std::collections::HashMap;
use std::path::Path;

use crate::i18n::{detect, filename::parse_filename_stem, path::ancestor_lang_from_path, Language};

/// One folder's cached inference result, plus the file-set identity it was
/// computed against.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct FolderLangEntry {
    /// Sorted source paths of every markdown file this folder contained
    /// when last inferred. The identity a rebuild compares against — NOT
    /// content, NOT mtimes: only membership can move this folder's
    /// language.
    files: Vec<String>,
    /// `Language::code()`, or `None` when no file in the folder yielded a
    /// confident detection (the folder contributes nothing; callers fall
    /// through to the site default same as an un-inferrable single page
    /// used to).
    lang: Option<String>,
}

/// Persisted `.moss/build.nosync/cache/folder-lang.json`. See the module doc for
/// why membership, not content, is the cache key.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct FolderLangCache {
    pub(super) entries: HashMap<String, FolderLangEntry>,
}

impl FolderLangCache {
    /// Load from disk. A missing or unparsable file is a cold cache, not an
    /// error — same fail-open convention as `FrontmatterScanCache::load`.
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to disk. Goes through `io_utils` because this file lives
    /// under `.moss/build.nosync/` (dataless is treated as absent there).
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        crate::build::io_utils::write_output(path, &json)
    }
}

/// The immediate containing folder of a markdown source path — `""` for a
/// root-level file. Every markdown file directly inside the same folder
/// shares one entry: `ancestor_lang_from_path` walks the SAME parent
/// components for all of them, so it can only ever agree.
pub(crate) fn folder_of(file_path: &str) -> String {
    match Path::new(file_path).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().replace('\\', "/"),
        _ => String::new(),
    }
}

/// Resolve the inferred language for every folder in `markdown_files` that
/// carries no language-naming convention. Folders a naming convention
/// already answers for (`ancestor_lang_from_path` is `Some`) get no entry
/// here — the caller doesn't need one, rung 3 is already satisfied by the
/// path alone. Returns `folder_path -> Language` for the rest, omitting any
/// folder whose files yielded no confident detection at all (callers fall
/// through to the site default, same as an undetectable single page used
/// to).
pub(crate) fn resolve_folder_languages(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    cache: &mut FolderLangCache,
    declarations: &super::frontmatter_cache::FrontmatterScanCache,
) -> HashMap<String, Language> {
    let is_evicted: &dyn Fn(&Path) -> bool = &crate::build::icloud::is_evicted;
    let mut by_folder: HashMap<String, Vec<&crate::types::content::FileInfo>> = HashMap::new();
    for file_info in markdown_files {
        if ancestor_lang_from_path(&file_info.path).is_some() {
            continue; // already named — no inference needed for this file's folder
        }
        // A filename suffix (post.zh-hans.md) also already names its language
        // — rung 2. Such a file neither needs the folder verdict nor may vote
        // in it: a translation's body voting would tip every partially-
        // translated folder toward the translation's language, turning e.g.
        // an English site with one index.zh-hans.md monolingual Chinese.
        let stem = Path::new(&file_info.path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if parse_filename_stem(stem).lang.is_some() {
            continue;
        }
        by_folder.entry(folder_of(&file_info.path)).or_default().push(file_info);
    }

    let mut result = HashMap::new();
    for (folder, files) in by_folder {
        // A declaration outranks any amount of inference.
        match declared_folder_lang(&files, declarations) {
            Declaration::Is(lang) => {
                result.insert(folder, lang);
                continue;
            }
            Declaration::Unrepresentable => continue,
            Declaration::None => {}
        }

        let mut current_files: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        current_files.sort();

        let unchanged = cache.entries.get(&folder).is_some_and(|e| e.files == current_files);

        let lang = if unchanged {
            cache.entries[&folder].lang.as_deref().and_then(Language::from_code)
        } else {
            let bodies: Vec<String> = files
                .iter()
                .filter_map(|f| {
                    let abs_path = source_path.join(&f.path);
                    if is_evicted(&abs_path) {
                        crate::build::cloud_readiness::request_download(&abs_path);
                        return None;
                    }
                    std::fs::read_to_string(&abs_path).ok().map(|c| detect::strip_frontmatter(&c))
                })
                .collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            // The vote itself answers "was there evidence at all" — `None`
            // when no file in the folder was long enough to vote or none that
            // was classified, so the folder contributes nothing and callers
            // fall through to the site default. This used to pre-scan every
            // body with `detect_language` to make that distinction, then throw
            // the result away and detect them all a second time; worse, the
            // pre-scan and the vote have since stopped agreeing — a folder of
            // stubs passes "something classified" and then votes to nothing,
            // which the old `unwrap_or(En)` would have called English.
            let inferred = detect::site_language_vote(&refs);

            cache.entries.insert(
                folder.clone(),
                FolderLangEntry {
                    files: current_files,
                    lang: inferred.map(|l| l.code().to_string()),
                },
            );
            inferred
        };

        if let Some(lang) = lang {
            result.insert(folder, lang);
        }
    }

    result
}

/// The `lang:` a folder's own index file declares — `index.md`, `README.md` or
/// the self-named `<folder>.md` — which speaks for every page in the folder.
///
/// Rung order for a page, top down: what the page says about ITSELF (its
/// frontmatter, its filename suffix) wins; then a folder literally NAMED after
/// a language, because a language TREE is a routing decision and the pages
/// under it are reached by that path; then this; then the inference below. In
/// other words a declaration outranks inference but not the tree it lives in —
/// which is why the caller never reaches this for a file under `zh-hant/`.
///
/// Only the languages moss ships are representable, because this rung feeds
/// `Language`. A folder declaring `fr` therefore gets NO folder-level answer at
/// all — it is a declaration, so it stops the inference from overruling it, and
/// its pages fall through to the site default (the same ceiling `<html lang>`
/// has, and tracked with it).
///
/// Read on every build rather than through the FILE-SET cache: that cache
/// exists so that editing a body cannot move a folder's language, and a `lang:`
/// in the folder's index is not a body — it is the author saying what the
/// folder is. It comes from the frontmatter-scan cache, which the page-map pass
/// has already filled for every file by the time this runs, so "every build"
/// costs a map lookup rather than a file read. It used to cost one read+parse
/// per folder: 118 of them on a 251-file vault, on every rebuild including a
/// one-file edit.
fn declared_folder_lang(
    files: &[&crate::types::content::FileInfo],
    declarations: &super::frontmatter_cache::FrontmatterScanCache,
) -> Declaration {
    // The canonical priority (bare index stems, then language-suffixed, then
    // the self-named folder note...) rather than "whichever file the scan
    // enumerated first" — a folder holding both `index.md` and `notes.md`
    // otherwise picked its declaring file by hash order.
    let names: Vec<&str> =
        files.iter().filter_map(|f| Path::new(&f.path).file_name()?.to_str()).collect();
    let folder_name = files
        .first()
        .and_then(|f| Path::new(&f.path).parent())
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let home = moss_core::home::detect_home_file_in_folder(&names, folder_name);
    let Some(index) = home.and_then(|name| files.iter().find(|f| f.path.ends_with(name))) else {
        return Declaration::None;
    };

    match declarations.declared_lang(&index.path) {
        // A code moss recognizes but has no `Language` for. The author still
        // declared something, so the inference must not overrule it.
        Some(code) => match Language::from_code(&code) {
            Some(lang) => Declaration::Is(lang),
            None => Declaration::Unrepresentable,
        },
        None => Declaration::None,
    }
}

/// What a folder's index says about the folder's language.
enum Declaration {
    /// Declared, and one moss can represent.
    Is(Language),
    /// Declared, but not one of moss's three — no folder answer, and no
    /// inference either, because guessing would overrule the author.
    Unrepresentable,
    /// Nothing declared: infer.
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::fixtures::SYNTHETIC_TRAD_CHINESE_ARTICLE_BODY;
    use crate::types::content::FileInfo;

    /// Every production caller resolves folder languages against a
    /// frontmatter-scan cache the page-map pass has already filled; these
    /// tests fill it the same way rather than hand-building entries.
    fn resolve(
        files: &[FileInfo],
        root: &Path,
        cache: &mut FolderLangCache,
    ) -> HashMap<String, Language> {
        let scanned =
            super::super::frontmatter_cache::FrontmatterScanCache::scanned_for_test(files, root);
        resolve_folder_languages(files, root, cache, &scanned)
    }

    fn file(path: &str) -> FileInfo {
        FileInfo { path: path.to_string(), file_type: "md".to_string(), size: 0, modified: None }
    }

    /// The invariant this module exists to hold: editing a folder's ONE
    /// file's body — even a realistic long-form article's real bytes, whose
    /// markup-heavy shape is exactly what used to flip the per-page
    /// detector — must not move the folder's resolved language, because a
    /// rebuild with the same file SET reuses the cached verdict without
    /// re-reading content at all.
    #[test]
    fn editing_a_realistic_article_does_not_move_its_folder_language() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("awards/writing/s2/rainy-season-letter");
        std::fs::create_dir_all(&folder).unwrap();
        let article_path = folder.join("雨季裡的一封信.md");
        std::fs::write(&article_path, SYNTHETIC_TRAD_CHINESE_ARTICLE_BODY).unwrap();

        let files = vec![file("awards/writing/s2/rainy-season-letter/雨季裡的一封信.md")];
        let mut cache = FolderLangCache::default();

        let before = resolve(&files, dir.path(), &mut cache);
        assert_eq!(
            before.get("awards/writing/s2/rainy-season-letter"),
            Some(&Language::ZhHant)
        );

        // Edit the body — same file, same folder membership — and rebuild
        // against the SAME (now-populated) cache, as a real second build
        // would.
        let edited = format!("{SYNTHETIC_TRAD_CHINESE_ARTICLE_BODY}\n\n<!-- rebuild-bench 1787295798955 -->\n");
        std::fs::write(&article_path, &edited).unwrap();

        let after = resolve(&files, dir.path(), &mut cache);
        assert_eq!(
            after.get("awards/writing/s2/rainy-season-letter"),
            Some(&Language::ZhHant),
            "a body edit with an unchanged file set must not move the folder's resolved language"
        );
    }

    /// The declaration rung, and the reason it sits outside the cache: an
    /// author who has to say what a folder is written in is contradicting the
    /// inference, so it must take effect on the next build — not on the next
    /// build that happens to add or remove a file.
    #[test]
    fn a_folder_index_declares_the_language_for_the_whole_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("notes")).unwrap();
        std::fs::write(
            dir.path().join("notes/one.md"),
            "This is a note about software engineering and web development, \
             written at some length so that it clears the floor and votes.",
        )
        .unwrap();
        let index = dir.path().join("notes/index.md");
        std::fs::write(&index, "---\ntitle: Notes\n---\n\nA folder of notes.").unwrap();

        let files = vec![file("notes/index.md"), file("notes/one.md")];
        let mut cache = FolderLangCache::default();

        let inferred = resolve(&files, dir.path(), &mut cache);
        assert_eq!(inferred.get("notes"), Some(&Language::En), "inference, with nothing declared");

        // Declare it. Same file set, and a warm cache holding the inferred
        // answer — the declaration must still win.
        std::fs::write(&index, "---\ntitle: Notes\nlang: zh-hant\n---\n\nA folder of notes.")
            .unwrap();
        let declared = resolve(&files, dir.path(), &mut cache);
        assert_eq!(
            declared.get("notes"),
            Some(&Language::ZhHant),
            "a declaration in the folder's index outranks what its files look like, \
             and is not held back by the file-set cache"
        );

        // A typo is not a fourth language: it must fall through to the
        // inference rather than silence the folder.
        std::fs::write(&index, "---\ntitle: Notes\nlang: english\n---\n\nA folder of notes.")
            .unwrap();
        let typo = resolve(&files, dir.path(), &mut cache);
        assert_eq!(typo.get("notes"), Some(&Language::En));
    }

    /// A folder with no naming convention and mixed-language files
    /// resolves to ONE language for the whole folder — every page in it
    /// gets the same fallback, not a per-page guess.
    #[test]
    fn mixed_language_folder_resolves_to_one_language_for_every_page() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("journal")).unwrap();
        std::fs::write(
            dir.path().join("journal/one.md"),
            "这是一篇关于软件工程和网页开发的中文文章。我们探讨了构建现代应用程序的各种技术。",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("journal/two.md"),
            "另一篇关于网页开发的中文文章，涵盖了现代框架和最佳实践，也讨论了软件工程方法论。",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("journal/three.md"),
            "This is a short English note about gardening in the spring.",
        )
        .unwrap();

        let files = vec![file("journal/one.md"), file("journal/two.md"), file("journal/three.md")];
        let mut cache = FolderLangCache::default();
        let langs = resolve(&files, dir.path(), &mut cache);

        // Chinese majority (2 of 3) wins the folder outright.
        assert_eq!(langs.get("journal"), Some(&Language::ZhHans));
        assert_eq!(langs.len(), 1, "one verdict for the whole folder, not one per file");
    }

    /// Rungs 1 and 2 are unchanged: a page's own frontmatter `lang:` and
    /// filename suffix still win over ANY folder signal, named or
    /// inferred. This is exercised at the `resolve_document_language`
    /// level (its own test suite in `i18n.rs`) — this test only confirms
    /// folder inference does not even attempt to cover a file whose
    /// ancestor already names a language, so the two sources can never
    /// disagree about who's authoritative.
    #[test]
    fn a_named_ancestor_folder_needs_no_inference() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("en")).unwrap();
        std::fs::write(dir.path().join("en/post.md"), "这是中文内容，但文件夹叫 en。").unwrap();

        let files = vec![file("en/post.md")];
        let mut cache = FolderLangCache::default();
        let langs = resolve(&files, dir.path(), &mut cache);

        assert!(
            langs.is_empty(),
            "a folder named after a language needs no inferred entry — ancestor_lang_from_path already answers it"
        );
    }

    /// Adding or removing a file IS allowed to change the folder's
    /// language — the mechanism isn't accidentally frozen forever, only
    /// stable against a body edit with the file set held constant.
    #[test]
    fn adding_a_file_changes_the_folder_language() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mixed")).unwrap();
        std::fs::write(dir.path().join("mixed/one.md"), "This is a short English note about the weather today.").unwrap();

        let mut cache = FolderLangCache::default();
        let files_before = vec![file("mixed/one.md")];
        let before = resolve(&files_before, dir.path(), &mut cache);
        assert_eq!(before.get("mixed"), Some(&Language::En));

        // Add two Chinese files — the file SET changes, so a recompute is
        // expected and the majority flips.
        std::fs::write(
            dir.path().join("mixed/two.md"),
            "这是一篇关于软件工程的中文文章，讨论了各种网页开发技术和方法。",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("mixed/three.md"),
            "另一篇关于编程的中文文章，涵盖了软件工程的最佳实践和框架选择。",
        )
        .unwrap();
        let files_after = vec![file("mixed/one.md"), file("mixed/two.md"), file("mixed/three.md")];
        let after = resolve(&files_after, dir.path(), &mut cache);
        assert_eq!(
            after.get("mixed"),
            Some(&Language::ZhHans),
            "adding files that change the folder's majority must be allowed to move its language"
        );
    }

    /// Removing a file down to nothing-but-undetectable content also
    /// updates the cached verdict — deletion is membership change too.
    #[test]
    fn removing_a_file_changes_the_folder_language() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("shrinking")).unwrap();
        std::fs::write(
            dir.path().join("shrinking/zh.md"),
            "这是一篇关于软件工程的中文文章，讨论了各种网页开发技术和方法论。",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("shrinking/en.md"),
            "This is an English note about software engineering and web development practices.",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("shrinking/en2.md"),
            "Another English note, this time about gardening and home improvement projects.",
        )
        .unwrap();

        let mut cache = FolderLangCache::default();
        let files_before =
            vec![file("shrinking/zh.md"), file("shrinking/en.md"), file("shrinking/en2.md")];
        let before = resolve(&files_before, dir.path(), &mut cache);
        assert_eq!(before.get("shrinking"), Some(&Language::En));

        // Remove both English files, leaving only the Chinese one.
        let files_after = vec![file("shrinking/zh.md")];
        let after = resolve(&files_after, dir.path(), &mut cache);
        assert_eq!(
            after.get("shrinking"),
            Some(&Language::ZhHans),
            "removing files down to a different majority must be allowed to move the folder's language"
        );
    }

    /// A translation named by filename suffix (index.zh-hans.md) must not
    /// vote in its folder's inference: its language is already answered by
    /// rung 2, and its body voting here tipped a partially-translated
    /// English site monolingual Chinese (caught by the nav-toggle-cluster
    /// render gate on the v0.11.6 cut).
    #[test]
    fn a_suffix_named_translation_does_not_vote_in_folder_inference() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("site")).unwrap();
        std::fs::write(
            dir.path().join("site/index.md"),
            "This is an English home page about software engineering and web development practices.",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("site/index.zh-hans.md"),
            "这是一篇关于软件工程和网页开发的中文文章。我们探讨了构建现代应用程序的各种技术。",
        )
        .unwrap();

        let files = vec![file("site/index.md"), file("site/index.zh-hans.md")];
        let mut cache = FolderLangCache::default();
        let langs = resolve(&files, dir.path(), &mut cache);

        assert_eq!(
            langs.get("site"),
            Some(&Language::En),
            "the suffixed translation's body must not move the folder's language"
        );
    }

    /// A folder with no confident detection at all contributes nothing —
    /// callers fall through to the site default, same as an undetectable
    /// single page used to.
    #[test]
    fn undetectable_folder_yields_no_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("terse")).unwrap();
        std::fs::write(dir.path().join("terse/a.md"), "hi").unwrap();
        std::fs::write(dir.path().join("terse/b.md"), "ok").unwrap();

        let files = vec![file("terse/a.md"), file("terse/b.md")];
        let mut cache = FolderLangCache::default();
        let langs = resolve(&files, dir.path(), &mut cache);

        assert!(langs.get("terse").is_none());
    }
}
