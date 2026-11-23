//! The words moss draws around a page, in each language a site can be in:
//! the language set, the string table and Chinese numerals. Pure tables, so
//! the build and every host draw the same words.

pub mod filename;
pub mod numerals;
pub mod path;
mod strings;

pub use strings::{diagram_type_name, hook_strings, sources_headings, t, term_root_title};

/// Strip a filename's language suffix and return just the clean stem.
///
/// For callers that only need the stem (e.g. URL slug computation, page-map
/// pre-scan) and don't care about the resolved language. Equivalent to
/// dropping the lang half of [`resolve_document_language`]'s return tuple,
/// without the noise of passing dummy site-default and ancestor-lang
/// arguments at the call site.
pub fn clean_stem_only(filename_stem: &str) -> String {
    filename::parse_filename_stem(filename_stem).stem
}

/// The folder rung of a page's language: the language a folder above the page
/// is named after, else `inferred`, the language the page's own folder
/// declares in its index or is inferred to be written in
/// (the build's folder-language pass). A slot-only file is
/// never a page, so it takes no inferred language: outside a language tree its
/// chrome is the site's.
pub fn folder_language(file_path: &str, inferred: Option<Language>, slot_only: bool) -> Option<Language> {
    path::ancestor_lang_from_path(file_path).or(if slot_only { None } else { inferred })
}

/// Resolve the language for a document using the priority chain:
/// 1. Explicit `lang` frontmatter value
/// 2. Filename language suffix (e.g. `post.zh-hans.md`)
/// 3. Ancestor folder lang (e.g. `en/post.md` inherits English from `en/`)
/// 4. Site default language (passed in)
///
/// A document's language is a pure function of its path plus its
/// frontmatter — it never reads the document's own body. Per-page content
/// detection would let a typo fix flip the language, feed a fingerprint, and
/// force a full site render.
///
/// `ancestor_lang` is precomputed by the caller and is a RICHER slot
/// than its name alone suggests: it's `Some` either because the folder is
/// literally *named* after a language (via
/// [`path::ancestor_lang_from_path`]), or because the folder carries no
/// naming convention and the caller inferred one FOR THE WHOLE FOLDER, ONCE,
/// in the scan/reduce phase (the build's folder-language pass) —
/// content detection still happens, just upstream of this function and
/// keyed to the folder's file *set* rather than any one page's bytes, so it
/// does not move on a body edit. This function does not need to know
/// which case it's in; both are "the folder says so," stronger than the
/// site default. Pass `None` when the caller has neither (e.g. most unit
/// tests, in-memory previews).
///
/// Returns `(resolved_language, clean_stem_without_suffix)`.
pub fn resolve_document_language(
    frontmatter_lang: Option<&str>,
    filename_stem: &str,
    site_default: Language,
    ancestor_lang: Option<Language>,
) -> (Language, String) {
    let parsed = filename::parse_filename_stem(filename_stem);

    // 1. Frontmatter lang (highest priority)
    if let Some(code) = frontmatter_lang {
        if let Some(lang) = Language::from_code(code) {
            return (lang, parsed.stem);
        }
    }

    // 2. Filename suffix
    if let Some(lang) = parsed.lang {
        return (lang, parsed.stem);
    }

    // 3. Ancestor folder — named, or folder-inferred (see doc comment above)
    if let Some(lang) = ancestor_lang {
        return (lang, parsed.stem);
    }

    // 4. Site default
    (site_default, parsed.stem)
}

/// Chinese numerals: a vertical-typesetting page in a CJK language. Counts,
/// dates, year headings and the series position all ask this one question.
pub fn use_cjk_numerals(typesetting: Option<&str>, lang: Language) -> bool {
    typesetting == Some("vertical") && lang.is_cjk()
}

/// Supported languages for UI string translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[default]
    En,
    ZhHans,
    ZhHant,
    De,
}

impl Language {
    /// Every interface language, so a caller that must cover them all
    /// (a per-language digest, a table test) reads one list.
    pub const ALL: [Language; 4] = [Language::En, Language::De, Language::ZhHans, Language::ZhHant];

    /// Returns true for CJK languages that benefit from Chinese numeral formatting.
    pub fn is_cjk(&self) -> bool {
        matches!(self, Language::ZhHans | Language::ZhHant)
    }

    /// Parse a language code string into a Language.
    ///
    /// Accepts codes like "en", "de", "zh-hans", "zh-hant" (case-insensitive).
    /// German is "de" and its regional forms "de-de", "de-at", "de-ch".
    /// Bare "zh" is treated as shorthand for Simplified Chinese (zh-hans);
    /// `zh-hant` / `zh-tw` remain distinct for Traditional Chinese.
    /// Returns None if the code is not recognized.
    pub fn from_code(code: &str) -> Option<Language> {
        match crate::home::folder_language_code(code) {
            Some("en") => Some(Language::En),
            Some("de") => Some(Language::De),
            Some("zh-hans") => Some(Language::ZhHans),
            Some("zh-hant") => Some(Language::ZhHant),
            _ => None,
        }
    }

    /// Parse an arbitrary BCP-47 language tag into a Language, never failing.
    ///
    /// Any `de` tag (`de-AT`, `de_CH`, `de-1996`) is German. Matches any `zh-*` tag by region into Simplified vs. Traditional
    /// (`zh-hk` / `zh-mo` are Traditional in practice). Unknown tags fall
    /// back to `En`. Use this when the input is untrusted or comes from
    /// web APIs (`document.documentElement.lang`, `Accept-Language`, HTML
    /// lang attributes, etc.); use `from_code` when you need to reject
    /// garbage.
    ///
    /// Must stay in sync with `langBucket()` in
    /// `js-src/site/subscribe/i18n.ts` — any reshape here needs a TS
    /// counterpart.
    ///
    /// This is the ONE place that buckets zh tags (see
    /// `tests/zh_bucketing_invariant_test.rs`). Everywhere else takes a
    /// `Language` and matches on it — never re-bucket from a `&str`.
    ///
    /// Parses by *subtags* (not a raw prefix) so it is robust to real-world
    /// locale strings: underscores (`zh_TW` from POSIX `LANG` / older Windows),
    /// a `Hant`/`Hans` script subtag in any position, the ISO-639-3 Sinitic
    /// language codes macOS/browsers occasionally report (`cmn`, `yue`, …), and
    /// a bare script subtag. Anything not recognizably Chinese or German → `En`.
    pub fn from_bcp47_lenient(tag: &str) -> Language {
        // Normalize: lowercase, and treat `_` as `-` so `zh_TW` still buckets as
        // Traditional instead of silently falling through to Simplified.
        let normalized = tag.to_ascii_lowercase().replace('_', "-");
        let subtags: Vec<&str> = normalized.split('-').filter(|s| !s.is_empty()).collect();
        let primary = subtags.first().copied().unwrap_or("");
        if primary == "de" || primary == "deu" {
            return Language::De;
        }

        // Primary-subtag codes that denote Chinese: the `zh` macrolanguage
        // (+ ISO-639-3 `zho`), the individual Sinitic languages that may be
        // reported, and a bare script subtag with no leading language.
        const SINITIC: [&str; 11] = [
            "zh", "zho", "cmn", "yue", "wuu", "nan", "hak", "gan", "hsn", "lzh", "czh",
        ];
        let is_chinese = SINITIC.contains(&primary) || primary == "hans" || primary == "hant";
        if !is_chinese {
            return Language::En;
        }

        // Within Chinese: Traditional if an explicit `Hant` script appears, or
        // (absent an explicit `Hans`) a Traditional-using region does. Otherwise
        // Simplified — the default for bare `zh`, `zh-CN`, `zh-SG`, etc.
        const HANT_REGIONS: [&str; 3] = ["tw", "hk", "mo"];
        let has_hant_script = subtags.contains(&"hant");
        let has_hans_script = subtags.contains(&"hans");
        let has_hant_region = subtags.iter().any(|s| HANT_REGIONS.contains(s));
        if has_hant_script || (!has_hans_script && has_hant_region) {
            Language::ZhHant
        } else {
            Language::ZhHans
        }
    }

    /// Returns the language code string (lowercase, hyphenated).
    ///
    /// This is the INTERNAL form — used as the TS `AppLocale` key and in moss's
    /// own routing. For a generated site's `<html lang>` / `hreflang` attributes
    /// use [`as_bcp47_attr`](Self::as_bcp47_attr), which is the canonical
    /// title-case form.
    pub fn code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::ZhHans => "zh-hans",
            Language::ZhHant => "zh-hant",
            Language::De => "de",
        }
    }

    /// Canonical BCP-47 tag with a TITLE-CASE script subtag (`zh-Hans` / `zh-Hant`)
    /// for emission into generated-site `<html lang>` and `hreflang` attributes.
    ///
    /// Kept distinct from [`code`](Self::code) (lowercase) on purpose: changing
    /// `code()` would break the lowercase TS `AppLocale` key. Browsers accept
    /// either case, but title-case is the BCP-47 canonical form and avoids SEO
    /// crawler warnings.
    pub fn as_bcp47_attr(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::ZhHans => "zh-Hans",
            Language::ZhHant => "zh-Hant",
            Language::De => "de",
        }
    }

    /// Returns a human-readable display name in the language's own script.
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::En => "EN",
            Language::ZhHans => "简",
            Language::ZhHant => "繁",
            Language::De => "DE",
        }
    }
}
