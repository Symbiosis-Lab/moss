//! Which language a folder of pages is written in: what its index declares,
//! else a vote over its pages' prose, where the host can detect languages.

use std::collections::HashMap;
use std::path::Path;

use super::Language;

/// Below this many characters of prose between them, documents give no answer.
/// 24 is about one Chinese sentence and a short English one. It bounds the
/// smallest corpus that gets an answer, not the smallest document that counts.
/// A floor on the total, not a test each document passes: thirty short posts
/// in one language answer together, where each alone would not.
pub const MIN_PROSE_CHARS: usize = 24;

/// One file's detected language, if any, and how many characters of prose
/// the detection read. A host with a language detector fills these; core has none.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Detected {
    pub lang: Option<String>,
    pub weight: usize,
}

/// The language documents are written in, each weighed by its prose, or `None`
/// when there is too little prose between them or none classified. A document
/// that did not classify still counts toward the floor: it is evidence of real
/// writing, not of which language. Ties break on the language code, so every
/// run gives the same answer.
pub fn language_vote(docs: impl IntoIterator<Item = (Option<Language>, usize)>) -> Option<Language> {
    let mut counts: HashMap<Language, usize> = HashMap::new();
    let mut total = 0usize;
    for (lang, weight) in docs {
        total += weight;
        if let Some(lang) = lang {
            *counts.entry(lang).or_insert(0) += weight;
        }
    }
    if total < MIN_PROSE_CHARS {
        return None;
    }
    counts.into_iter()
        .max_by(|(a, wa), (b, wb)| wa.cmp(wb).then_with(|| b.code().cmp(a.code())))
        .map(|(lang, _)| lang)
}

/// What a folder's index says about the folder's language.
enum Declaration {
    Is(Language),
    /// A known code moss has no interface for: no answer, and no vote either,
    /// because a guess would overrule the author.
    Unrepresentable,
    None,
}

/// Each folder's language where its path does not name one. The folder's
/// index `lang:` speaks first; with none, `vote(folder, files)` runs over the
/// folder's files, sorted, unless the site's language is declared, which then
/// holds. A file under a language-named folder, with a language suffix, or
/// slot-only neither votes nor needs an answer. `declared_lang` is a file's
/// `lang:` when it is a known code. A folder with no answer is left out, and
/// its pages take the site language. Paths are site-relative and `/`-separated,
/// and so are the returned folder keys.
pub fn folder_languages<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    declared_lang: &dyn Fn(&str) -> Option<String>,
    slot_only: &dyn Fn(&str) -> bool,
    site_lang_declared: bool,
    vote: &mut dyn FnMut(&str, &[&'a str]) -> Option<Language>,
) -> HashMap<String, Language> {
    let mut by_folder: HashMap<&str, Vec<&str>> = HashMap::new();
    for path in paths {
        let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if super::path::ancestor_lang_from_path(path).is_some()
            || super::filename::parse_filename_stem(stem).lang.is_some()
            || slot_only(path)
        {
            continue;
        }
        by_folder.entry(crate::resolve::parent_dir(path)).or_default().push(path);
    }
    let mut result = HashMap::new();
    for (folder, mut files) in by_folder {
        let lang = match declared_folder_lang(folder, &files, declared_lang) {
            Declaration::Is(lang) => Some(lang),
            Declaration::Unrepresentable => None,
            Declaration::None if site_lang_declared => None,
            Declaration::None => {
                files.sort_unstable();
                vote(folder, &files)
            }
        };
        if let Some(lang) = lang {
            result.insert(folder.to_string(), lang);
        }
    }
    result
}

/// The `lang:` of the folder's home file (`index.md`, `README.md`, the
/// self-named `<folder>.md`, in the home election's order).
fn declared_folder_lang(folder: &str, files: &[&str], declared_lang: &dyn Fn(&str) -> Option<String>) -> Declaration {
    let names: Vec<&str> = files.iter().filter_map(|f| Path::new(f).file_name()?.to_str()).collect();
    let folder_name = Path::new(folder).file_name().and_then(|s| s.to_str()).unwrap_or("");
    let Some(index) = crate::home::detect_home_file_in_folder(&names, folder_name)
        .and_then(|name| files.iter().find(|f| Path::new(f).file_name().and_then(|n| n.to_str()) == Some(name)))
    else {
        return Declaration::None;
    };
    match declared_lang(index) {
        Some(code) => Language::from_code(&code).map_or(Declaration::Unrepresentable, Declaration::Is),
        None => Declaration::None,
    }
}

#[cfg(test)]
#[path = "vote_tests.rs"]
mod tests;
