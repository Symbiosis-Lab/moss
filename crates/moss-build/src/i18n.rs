//! Internationalization (i18n) support for moss
//!
//! Provides language detection, UI string translation, and translation linking
//! for multilingual static sites.
//!
//! # Picking the right entry point
//!
//! - [`resolve_document_language`] — 4-step priority chain (frontmatter >
//!   filename suffix > ancestor folder > site default). Returns
//!   `(Language, clean_stem)`. Use this from the markdown pipeline where
//!   the doc's resolved language matters downstream (template rendering,
//!   `<html lang>`, language switcher, per-lang grouping). This
//!   is a pure function of the doc's path and frontmatter — it never
//!   reads the body. Content-based inference still happens, but for a
//!   whole FOLDER at once (`build::scan::page_map::folder_lang`), and the
//!   result is folded into the `ancestor_lang` argument by the caller.
//! - [`clean_stem_only`] — just strips the language suffix from a
//!   filename stem, returning the base name. Use this from pre-scan
//!   passes (e.g. `build_page_map`) that compute URL slugs and don't
//!   care about the resolved language; the full chain runs once per
//!   doc later in the pipeline regardless.
//!
//! Misusing the two: calling `clean_stem_only` where you need the
//! resolved lang gives you a `String` only and the bug is loud at the
//! call site. Calling `resolve_document_language` in a pre-scan that
//! only wants the stem just costs a wasted whatlang invocation; the
//! returned stem is identical.

pub mod detect;
pub mod filename;
pub mod link;
pub mod numerals;
pub mod path;
pub mod site_languages;
mod strings;

/// Supported languages for UI string translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    #[default]
    En,
    ZhHans,
    ZhHant,
}

impl Language {
    /// Returns true for CJK languages that benefit from Chinese numeral formatting.
    pub fn is_cjk(&self) -> bool {
        matches!(self, Language::ZhHans | Language::ZhHant)
    }

    /// Parse a language code string into a Language.
    ///
    /// Accepts codes like "en", "zh-hans", "zh-hant" (case-insensitive).
    /// Bare "zh" is treated as shorthand for Simplified Chinese (zh-hans);
    /// `zh-hant` / `zh-tw` remain distinct for Traditional Chinese.
    /// Returns None if the code is not recognized.
    pub fn from_code(code: &str) -> Option<Language> {
        match code.to_lowercase().as_str() {
            "en" => Some(Language::En),
            "zh" | "zh-hans" | "zh-cn" => Some(Language::ZhHans),
            "zh-hant" | "zh-tw" => Some(Language::ZhHant),
            _ => None,
        }
    }

    /// Parse an arbitrary BCP-47 language tag into a Language, never failing.
    ///
    /// Matches any `zh-*` tag by region into Simplified vs. Traditional
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
    /// a bare script subtag. Anything not recognizably Chinese → `En` (moss's
    /// only non-Chinese UI language).
    pub fn from_bcp47_lenient(tag: &str) -> Language {
        // Normalize: lowercase, and treat `_` as `-` so `zh_TW` still buckets as
        // Traditional instead of silently falling through to Simplified.
        let normalized = tag.to_ascii_lowercase().replace('_', "-");
        let subtags: Vec<&str> = normalized.split('-').filter(|s| !s.is_empty()).collect();
        let primary = subtags.first().copied().unwrap_or("");

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
        let has_hant_script = subtags.iter().any(|s| *s == "hant");
        let has_hans_script = subtags.iter().any(|s| *s == "hans");
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
        }
    }

    /// Returns a human-readable display name in the language's own script.
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::En => "EN",
            Language::ZhHans => "简",
            Language::ZhHant => "繁",
        }
    }
}

pub use strings::{t, term_root_title};

/// A folder card's article count as displayed text — `3 articles`, `三篇`.
///
/// Chinese numerals only under vertical CJK: an Arabic digit in a
/// vertical run is laid on its side.
pub fn article_count_label(lang: Language, count: usize, typesetting: Option<&str>) -> String {
    let counter = if count == 1 {
        t(lang, "article_counter_singular")
    } else {
        t(lang, "article_counter")
    };
    if use_cjk_numerals(typesetting, lang) {
        format!("{}{}", numerals::to_chinese_numeral(count), counter)
    } else {
        format!("{} {}", count, counter)
    }
}

/// The one predicate that decides whether a piece of UI chrome is written in
/// Chinese numerals: a vertical-typesetting page in a CJK language.
///
/// Lives here rather than beside the date formatters because the counts, the
/// dates, the year headings and the series position all ask the same question,
/// and `build::components::date` re-exports it for the callers that reach it
/// by that path. It used to be spelled out a second time inside
/// [`article_count_label`]; a card count and a card date could then disagree.
pub fn use_cjk_numerals(typesetting: Option<&str>, lang: Language) -> bool {
    typesetting == Some("vertical") && lang.is_cjk()
}

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

/// Resolve the language for a document using the priority chain:
/// 1. Explicit `lang` frontmatter value
/// 2. Filename language suffix (e.g. `post.zh-hans.md`)
/// 3. Ancestor folder lang (e.g. `en/post.md` inherits English from `en/`)
/// 4. Site default language (passed in)
///
/// A document's language is a pure function of its path plus its
/// frontmatter — it no longer reads the document's own body at all. There
/// used to be a fifth rung here that called `detect::detect_language` on
/// `content` directly, per page, on every build; a content edit could flip
/// it, which fed a fingerprint and forced a full site render over a typo
/// fix. It is gone.
///
/// `ancestor_lang` is precomputed by the caller and is now a RICHER slot
/// than its name alone suggests: it's `Some` either because the folder is
/// literally *named* after a language (via
/// [`path::ancestor_lang_from_path`]), or because the folder carries no
/// naming convention and the caller inferred one FOR THE WHOLE FOLDER, ONCE,
/// in the scan/reduce phase (`build::scan::page_map::folder_lang`) —
/// content detection still happens, just upstream of this function and
/// keyed to the folder's file *set* rather than any one page's bytes, so it
/// no longer moves on a body edit. This function does not need to know
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

/// The `<html lang>` tag for a language CODE — the one canonicalizer, so a
/// site's declaration and a page's declaration cannot disagree about the same
/// string. `zh-tw` must emit `zh-Hant`, the edition moss ships, not the region
/// `zh-TW`; only a code with no `Language` variant takes the general rule.
pub fn lang_tag(code: &str) -> String {
    match Language::from_code(code) {
        Some(lang) => lang.as_bcp47_attr().to_string(),
        None => canonical_bcp47(code),
    }
}

/// The chip a language switcher shows for a declared tag.
///
/// The three languages moss translates its interface into keep their own short
/// labels (`繁`, `简`, `EN`) — they are what the existing switcher shows and
/// changing them would change every multilingual moss site. Every other
/// declared language gets its endonym, because a reader looking for the German
/// edition is looking for "Deutsch"; `DE` would be a code, not a name.
///
/// Total for any tag that passed the allowlist, which is every tag that can
/// reach a switcher.
pub fn switcher_label(tag: &str) -> &'static str {
    match Language::from_code(tag) {
        Some(lang) => lang.display_name(),
        None => moss_core::home::endonym(tag).unwrap_or(""),
    }
}

/// Do two declared tags name the same EDITION of a site — the thing an
/// `hreflang="x-default"` points at?
///
/// RFC 4647 basic filtering: one tag is a prefix of the other at a subtag
/// boundary, so an `en` page is the x-default of an `en-GB` site. That pairing
/// is the whole reason this is not `==`: `en-gb` is in `KNOWN_LANG_SUFFIXES`
/// but [`Language::from_code`] rejects region subtags, so it reaches here as
/// `canonical_bcp47`'s `en-GB` and nothing else would match it to `en`.
///
/// Comparing `Language` variants instead would be wrong twice over. It cannot
/// see `en`/`en-GB` at all, and it would call `zh-Hans` and `zh-Hant` one
/// edition — two editions that must never stand in as each other's x-default.
/// It is also unnecessary: every tag reaching here has been through
/// [`lang_tag`], which maps each code a `Language` variant recognizes onto one
/// canonical spelling, so `zh-tw` is already `zh-Hant` by this point and two
/// tags naming one variant are the same string.
pub(crate) fn same_language_edition(a: &str, b: &str) -> bool {
    let prefixes = |short: &str, long: &str| {
        long.len() > short.len()
            && long.as_bytes()[short.len()] == b'-'
            && long[..short.len()].eq_ignore_ascii_case(short)
    };
    a.eq_ignore_ascii_case(b) || prefixes(a, b) || prefixes(b, a)
}

/// Canonicalize a BCP-47 tag for emission: lowercase language, `Titlecase`
/// script, `UPPERCASE` region. `zh-hant` → `zh-Hant`, `pt-br` → `pt-BR`.
///
/// Browsers accept any case, but the canonical form is what avoids SEO crawler
/// warnings — the same reason [`Language::as_bcp47_attr`] is title-cased by
/// hand. This is the general form of that, for the languages moss recognizes
/// but has no `Language` variant for.
pub(crate) fn canonical_bcp47(code: &str) -> String {
    code.split('-')
        .enumerate()
        .map(|(i, part)| {
            let lower = part.to_ascii_lowercase();
            match (i, part.len()) {
                (0, _) => lower,
                // A 4-letter subtag is a script (`hant`), a 2-letter one a
                // region (`br`). Anything else (a variant like `1901`) is left
                // as it came.
                (_, 4) => {
                    let mut c = lower.chars();
                    c.next()
                        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                        .unwrap_or(lower)
                }
                (_, 2) => lower.to_ascii_uppercase(),
                _ => lower,
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}
/// What this PAGE's language is, canonically — from its own declaration or,
/// failing that, from the folder it sits in — or `None` when neither says
/// anything and the SITE default is the answer.
///
/// `None` used to mean two different things: "nothing here" and "declares one
/// of moss's three, so read it off the UI language instead". Every emission
/// site therefore had to keep a `ui_lang` fallback that could not tell those
/// apart, which is how a site declaring `fr` served pages saying `en`.
/// One meaning now: nothing at this page or its folder, use the site's.
pub fn declared_lang_tag(
    frontmatter_lang: Option<&str>,
    filename_stem: &str,
    file_path: &str,
    ancestor_lang: Option<Language>,
) -> Option<String> {
    declared_only(frontmatter_lang, filename_stem, file_path)
        // The folder-inferred rung — evidence about this page's
        // neighbourhood, and the last one that is about the page at all.
        .or_else(|| ancestor_lang.map(|l| l.as_bcp47_attr().to_string()))
}

/// The `lang:` a markdown file's frontmatter declares, validated against the
/// allowlist — the one reader for every rung that asks a FILE what language it
/// claims to be, whether the answer is about that file's own page (the
/// homepage rung in `detect`) or about the folder around it (`folder_lang`).
///
/// Validated here rather than at each caller because this string reaches
/// `<html lang="...">` unescaped: `is_known_language_code` is a closed
/// allowlist, so what comes out cannot open an attribute, and `lang: english`
/// is a typo that falls through rather than an invented fourth language.
///
/// Eviction-guarded: every caller is on the render path of every build, and an
/// unguarded read of a dataless `index.md` would park the whole build on the OS
/// materializing it.
pub fn declared_lang_in_file(
    path: &std::path::Path,
    is_evicted: &dyn Fn(&std::path::Path) -> bool,
) -> Option<String> {
    if is_evicted(path) {
        crate::build::cloud_readiness::request_download(path);
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let declared = crate::build::scan::article_map::parse_frontmatter(&content)
        .get("lang")?
        .as_str()?
        .trim()
        .to_string();
    moss_core::home::is_known_language_code(&declared).then_some(declared)
}

/// The tag for a page that declares nothing of its own but sits in a folder
/// whose language IS known — a synthesized folder index. Total by
/// construction: an authored page can say "nothing here, ask the site"
/// (`declared_lang_tag`'s `None`), a synthesized one cannot, because the
/// folder it is synthesized for is the answer.
pub fn lang_tag_in_tree(file_path: &str, folder_lang: Language) -> String {
    declared_only(None, "", file_path)
        .unwrap_or_else(|| folder_lang.as_bcp47_attr().to_string())
}

/// The canonical tag this page DECLARES — frontmatter, then filename suffix,
/// then language-tree directory — with no fallback of any kind.
fn declared_only(
    frontmatter_lang: Option<&str>,
    filename_stem: &str,
    file_path: &str,
) -> Option<String> {
    let declared = frontmatter_lang
        .map(str::to_string)
        .or_else(|| moss_core::home::lang_suffix(filename_stem).map(str::to_string))
        .or_else(|| moss_core::home::lang_tree_prefix(file_path).map(str::to_string));

    if let Some(declared) = declared {
        // Unrecognized frontmatter (`lang: english`) is not a fourth language,
        // it is a typo — fall through rather than invent `lang="english"`.
        if moss_core::home::is_known_language_code(&declared) {
            return Some(lang_tag(&declared));
        }
    }

    None
}

/// Test fixtures shared across the i18n test suite AND
/// `build::scan::page_map::folder_lang`'s tests (folder-level
/// inference moved content detection out of this module, but its
/// stability proof still needs the same real bytes). A separate,
/// `pub(crate)` module rather than living inside `mod tests` below, which
/// only this crate's own tests can see across a file boundary.
#[cfg(test)]
pub(crate) mod fixtures {
    /// A synthetic article body shaped like a real vault article, byte-for-byte
    /// stable (frontmatter stripped). Chinese prose is a MINORITY of this
    /// text by raw byte count — `:::hero`/`:::grid` shortcode directives,
    /// `![[assets/…jpg]]` wikilink targets and `/reviews/…` link
    /// destinations outweigh it — which is exactly the shape that used to
    /// flip a page like this to English on a trivial edit and force a
    /// 223-page full rebuild for a 1-page change.
    pub(crate) const SYNTHETIC_TRAD_CHINESE_ARTICLE_BODY: &str = "\n\n:::hero {image=letters-in-rain-cover.jpg}\n:::\n\n一座河灣小城的書信資料館收下了一位隱居作家的信件；一位方言譯者沿著河口走了三十幾天；渡口的往事在一個雨季裡靜靜歸來。老河道改道之後，河灣人如何重新找回自己的方言、故事與身份，而所謂「返鄉寫作」又在日常裡長成什麼模樣。\n\n## 全文\n\n:::grid 3 {.fs-parts}\n![[assets/7c14f2a03df8b562.jpg]]\n\n上篇\n\n### [在河灣，一座書信資料館的收藏](/reviews/fiction/edition-2/letters-in-rain/archive-riverbend/)\n+++\n![[assets/91ab6de204f7c318.jpg]]\n\n中篇\n\n### [在河口的兩端之間往返](/reviews/fiction/edition-2/letters-in-rain/between-the-banks/)\n+++\n![[assets/5f309c81b6e42a97.jpg]]\n\n下篇\n\n### [往事在雨季裡靜靜回來](/reviews/fiction/edition-2/letters-in-rain/the-old-crossing/)\n:::\n\n## 場外\n\n- [場外手記：走進雨季很容易，但走出來很難](/reviews/fiction/edition-2/letters-in-rain/memo/)\n- [編輯手記：周一 x 陳遠山](/reviews/fiction/edition-2/letters-in-rain/editor-memo/)\n- [發佈會記錄：陳遠山 x 吳夏 x 周一 | 雨季裡的文學選擇：老河道改道之後](/reviews/fiction/edition-2/letters-in-rain/launch/)\n";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_code_english() {
        assert_eq!(Language::from_code("en"), Some(Language::En));
        assert_eq!(Language::from_code("EN"), Some(Language::En));
    }

    #[test]
    fn test_language_from_code_chinese_simplified() {
        assert_eq!(Language::from_code("zh-hans"), Some(Language::ZhHans));
        assert_eq!(Language::from_code("zh-cn"), Some(Language::ZhHans));
        assert_eq!(Language::from_code("ZH-HANS"), Some(Language::ZhHans));
    }

    #[test]
    fn test_language_from_code_zh_shorthand() {
        // Bare `zh` normalizes to Simplified Chinese (Task 2.6)
        assert_eq!(Language::from_code("zh"), Some(Language::ZhHans));
        assert_eq!(Language::from_code("ZH"), Some(Language::ZhHans));
    }

    #[test]
    fn test_language_from_code_chinese_traditional() {
        assert_eq!(Language::from_code("zh-hant"), Some(Language::ZhHant));
        assert_eq!(Language::from_code("zh-tw"), Some(Language::ZhHant));
    }

    #[test]
    fn test_language_from_code_unknown() {
        assert_eq!(Language::from_code("fr"), None);
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::from_code("xyz"), None);
    }

    #[test]
    fn test_language_code_roundtrip() {
        assert_eq!(Language::from_code(Language::En.code()), Some(Language::En));
        assert_eq!(
            Language::from_code(Language::ZhHans.code()),
            Some(Language::ZhHans)
        );
        assert_eq!(
            Language::from_code(Language::ZhHant.code()),
            Some(Language::ZhHant)
        );
    }

    #[test]
    fn from_bcp47_lenient_standard_tags() {
        // The common macOS `sys-locale` outputs and web inputs.
        for tag in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh-SG", "zh_CN"] {
            assert_eq!(Language::from_bcp47_lenient(tag), Language::ZhHans, "{tag}");
        }
        for tag in ["zh-Hant", "zh-TW", "zh-HK", "zh-MO", "zh-Hant-TW"] {
            assert_eq!(Language::from_bcp47_lenient(tag), Language::ZhHant, "{tag}");
        }
        for tag in ["en", "en-US", "", "fr", "de-DE"] {
            assert_eq!(Language::from_bcp47_lenient(tag), Language::En, "{tag}");
        }
    }

    #[test]
    fn from_bcp47_lenient_underscore_traditional_no_longer_mis_buckets() {
        // Regression: POSIX/older-Windows underscore forms must NOT collapse to
        // Simplified. Before the subtag parser these fell through to ZhHans.
        assert_eq!(Language::from_bcp47_lenient("zh_TW"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh_HK"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh_Hant"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh_CN"), Language::ZhHans);
    }

    #[test]
    fn from_bcp47_lenient_sinitic_codes_are_chinese_not_english() {
        // ISO-639-3 Sinitic codes previously fell through to English.
        assert_eq!(Language::from_bcp47_lenient("cmn"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("cmn-Hans-CN"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("zho"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("yue-HK"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("yue-Hant"), Language::ZhHant);
    }

    #[test]
    fn from_bcp47_lenient_bare_script_subtag() {
        // A script-only tag (no leading language) still buckets by script.
        assert_eq!(Language::from_bcp47_lenient("Hans"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("Hant"), Language::ZhHant);
        // An explicit Hans script wins over a Traditional-region subtag.
        assert_eq!(Language::from_bcp47_lenient("zh-Hans-HK"), Language::ZhHans);
    }

    #[test]
    fn test_language_default_is_english() {
        assert_eq!(Language::default(), Language::En);
    }

    #[test]
    fn test_display_names() {
        assert_eq!(Language::En.display_name(), "EN");
        assert_eq!(Language::ZhHans.display_name(), "简");
        assert_eq!(Language::ZhHant.display_name(), "繁");
    }

    #[test]
    fn test_as_bcp47_attr_is_title_case() {
        // Internal `code()` stays lowercase (TS key); the attr form is title-case.
        assert_eq!(Language::En.as_bcp47_attr(), "en");
        assert_eq!(Language::ZhHans.as_bcp47_attr(), "zh-Hans");
        assert_eq!(Language::ZhHant.as_bcp47_attr(), "zh-Hant");
        assert_eq!(Language::ZhHans.code(), "zh-hans"); // unchanged
    }

    // String table tests
    #[test]
    fn test_t_month_names_english() {
        assert_eq!(t(Language::En, "month_1"), "January");
        assert_eq!(t(Language::En, "month_12"), "December");
    }

    #[test]
    fn test_t_month_names_chinese_simplified() {
        assert_eq!(t(Language::ZhHans, "month_1"), "1月");
        assert_eq!(t(Language::ZhHans, "month_12"), "12月");
    }

    #[test]
    fn test_t_month_names_chinese_traditional() {
        assert_eq!(t(Language::ZhHant, "month_1"), "1月");
        assert_eq!(t(Language::ZhHant, "month_12"), "12月");
    }

    #[test]
    fn test_t_month_abbrev_english() {
        assert_eq!(t(Language::En, "month_abbr_1"), "Jan");
        assert_eq!(t(Language::En, "month_abbr_12"), "Dec");
    }

    #[test]
    fn test_t_month_abbrev_chinese() {
        // Chinese uses same format for abbreviated months
        assert_eq!(t(Language::ZhHans, "month_abbr_1"), "1月");
        assert_eq!(t(Language::ZhHant, "month_abbr_1"), "1月");
    }

    #[test]
    fn test_t_article_counter() {
        assert_eq!(t(Language::En, "article_counter"), "articles");
        assert_eq!(t(Language::ZhHans, "article_counter"), "篇");
        assert_eq!(t(Language::ZhHant, "article_counter"), "篇");
    }

    #[test]
    fn test_t_toggle_dark_mode() {
        assert_eq!(t(Language::En, "toggle_dark_mode"), "Toggle dark mode");
        assert_eq!(t(Language::ZhHans, "toggle_dark_mode"), "切换深色模式");
    }

    #[test]
    fn test_t_subscribe_via_rss() {
        assert_eq!(t(Language::En, "subscribe_via_rss"), "Subscribe via RSS");
        assert_eq!(t(Language::ZhHans, "subscribe_via_rss"), "通过 RSS 订阅");
        assert_eq!(t(Language::ZhHant, "subscribe_via_rss"), "透過 RSS 訂閱");
    }

    #[test]
    fn test_t_media_page_titles() {
        assert_eq!(t(Language::En, "photography"), "Photography");
        assert_eq!(t(Language::ZhHans, "photography"), "摄影");
        assert_eq!(t(Language::ZhHant, "photography"), "攝影");

        assert_eq!(t(Language::En, "videos"), "Videos");
        assert_eq!(t(Language::ZhHans, "videos"), "视频");
        assert_eq!(t(Language::ZhHant, "videos"), "影片");

        assert_eq!(t(Language::En, "experiments"), "Experiments");
        assert_eq!(t(Language::ZhHans, "experiments"), "实验");
        assert_eq!(t(Language::ZhHant, "experiments"), "實驗");
    }

    #[test]
    fn test_t_lightbox_controls() {
        assert_eq!(t(Language::En, "close"), "Close");
        assert_eq!(t(Language::En, "previous"), "Previous");
        assert_eq!(t(Language::En, "next"), "Next");
        assert_eq!(t(Language::En, "view_in_article"), "View in article");

        assert_eq!(t(Language::ZhHans, "close"), "关闭");
        assert_eq!(t(Language::ZhHans, "previous"), "上一个");
        assert_eq!(t(Language::ZhHans, "next"), "下一个");
        assert_eq!(t(Language::ZhHans, "view_in_article"), "在文章中查看");
    }

    #[test]
    fn test_t_defaults() {
        assert_eq!(t(Language::En, "untitled"), "Untitled");
        assert_eq!(t(Language::ZhHans, "untitled"), "无标题");
        assert_eq!(t(Language::ZhHant, "untitled"), "無標題");

        assert_eq!(t(Language::En, "untitled_site"), "Untitled Site");
        assert_eq!(t(Language::ZhHans, "untitled_site"), "无标题站点");
    }

    #[test]
    fn test_t_unknown_key_returns_key() {
        assert_eq!(t(Language::En, "nonexistent_key"), "nonexistent_key");
    }

    #[test]
    fn test_t_unknown_fallback() {
        assert_eq!(t(Language::En, "unknown"), "Unknown");
        assert_eq!(t(Language::ZhHans, "unknown"), "未知");
    }

    // Comment strings
    #[test]
    fn test_t_comment_strings_english() {
        assert_eq!(t(Language::En, "comment_count_zero"), "Comments");
        assert_eq!(t(Language::En, "comment_count_one"), "1 comment");
        assert_eq!(t(Language::En, "comment_count_many"), "{} comments");
        assert_eq!(
            t(Language::En, "comment_placeholder"),
            "Leave your thoughts"
        );
        assert_eq!(t(Language::En, "comment_name"), "Name");
        assert_eq!(t(Language::En, "comment_email"), "Email");
        assert_eq!(t(Language::En, "comment_website"), "Website (optional)");
        assert_eq!(t(Language::En, "comment_reply"), "Reply");
    }

    #[test]
    fn test_t_comment_strings_zh_hans() {
        assert_eq!(t(Language::ZhHans, "comment_count_zero"), "评论");
        assert_eq!(t(Language::ZhHans, "comment_count_one"), "1条评论");
        assert_eq!(t(Language::ZhHans, "comment_count_many"), "{}条评论");
        assert_eq!(t(Language::ZhHans, "comment_placeholder"), "留下你的想法");
        assert_eq!(t(Language::ZhHans, "comment_name"), "名字");
        assert_eq!(t(Language::ZhHans, "comment_email"), "邮箱");
        assert_eq!(t(Language::ZhHans, "comment_website"), "网站（选填）");
        assert_eq!(t(Language::ZhHans, "comment_reply"), "回复");
    }

    #[test]
    fn test_t_comment_strings_zh_hant() {
        assert_eq!(t(Language::ZhHant, "comment_count_zero"), "留言");
        assert_eq!(t(Language::ZhHant, "comment_count_one"), "1則留言");
        assert_eq!(t(Language::ZhHant, "comment_count_many"), "{}則留言");
        assert_eq!(t(Language::ZhHant, "comment_placeholder"), "留下你的想法");
        assert_eq!(t(Language::ZhHant, "comment_name"), "名字");
        assert_eq!(t(Language::ZhHant, "comment_email"), "電子郵件");
        assert_eq!(t(Language::ZhHant, "comment_website"), "網站（選填）");
        assert_eq!(t(Language::ZhHant, "comment_reply"), "回覆");
    }

    #[test]
    fn test_t_available_after_publishing() {
        assert_eq!(
            t(Language::En, "available_after_publishing"),
            "Available after publishing"
        );
        assert_eq!(
            t(Language::ZhHans, "available_after_publishing"),
            "发布后启用"
        );
        assert_eq!(
            t(Language::ZhHant, "available_after_publishing"),
            "發佈後啟用"
        );
    }

    // resolve_document_language tests. This module dropped rung 4 (per-page
    // content detection) from this function entirely — it is now a pure
    // function of path + frontmatter, so `content` is no longer a
    // parameter. Tests that only existed to exercise that removed rung
    // (content beating/losing to some other rung) went with it; the
    // content-inference behavior they covered now lives at the FOLDER
    // level in `build::scan::page_map::folder_lang`, tested there against
    // `fixtures::SYNTHETIC_TRAD_CHINESE_ARTICLE_BODY`.
    #[test]
    fn test_resolve_frontmatter_wins() {
        let (lang, stem) = resolve_document_language(
            Some("zh-hans"),
            "my-post.en", // filename says English
            Language::En,
            Some(Language::ZhHant), // ancestor would say Traditional
        );
        assert_eq!(lang, Language::ZhHans); // frontmatter wins over everything else
        assert_eq!(stem, "my-post"); // suffix stripped
    }

    #[test]
    fn test_resolve_filename_suffix_wins_over_ancestor() {
        let (lang, stem) = resolve_document_language(
            None,
            "my-post.zh-hant",
            Language::En,
            Some(Language::En), // ancestor says English (e.g. en/my-post.zh-hant.md)
        );
        assert_eq!(lang, Language::ZhHant); // filename suffix is more specific
        assert_eq!(stem, "my-post");
    }

    #[test]
    fn test_resolve_ancestor_wins_over_site_default() {
        // The case the user actually hit: a short article in a folder
        // named for the OTHER language than the site default. Folder
        // placement — named or inferred — beats the site default.
        let (lang, stem) = resolve_document_language(
            None,
            "winter-song",
            Language::ZhHans, // site is Chinese-default
            Some(Language::En),
        );
        assert_eq!(lang, Language::En); // ancestor overrides site default
        assert_eq!(stem, "winter-song");
    }

    #[test]
    fn test_resolve_falls_back_to_site_default() {
        let (lang, stem) = resolve_document_language(
            None,
            "x", // no suffix
            Language::ZhHant,
            None, // no ancestor, no folder inference either
        );
        assert_eq!(lang, Language::ZhHant); // site default
        assert_eq!(stem, "x");
    }

    #[test]
    fn test_resolve_invalid_frontmatter_lang_falls_through() {
        let (lang, stem) = resolve_document_language(
            Some("invalid-code"),
            "post.zh-hans",
            Language::En,
            None,
        );
        assert_eq!(lang, Language::ZhHans); // falls to filename
        assert_eq!(stem, "post");
    }

    // ---- declared_lang_tag: document language vs UI language ----

    /// The bug this split fixed: a `ja/` tree resolved to the site default and
    /// `<html lang>` was written from it, so a Japanese page announced itself
    /// as Traditional Chinese to every screen reader and crawler.
    #[test]
    fn a_language_tree_outside_the_ui_three_declares_its_own_language() {
        assert_eq!(declared_lang_tag(None, "ja", "ja/ja.md", None), Some("ja".to_string()));
        assert_eq!(declared_lang_tag(None, "index", "fr/index.md", None), Some("fr".to_string()));
        assert_eq!(declared_lang_tag(None, "post", "de/blog/post.md", None), Some("de".to_string()));
    }

    /// The three that have interface strings canonicalize through `Language`,
    /// not the general rule — `zh-tw/` is the Traditional EDITION, so it must
    /// emit `zh-Hant` and never the region `zh-TW`. They used to return `None`
    /// here and be recovered from the UI language at the emission site, which
    /// is what made `None` ambiguous.
    #[test]
    fn the_three_ui_languages_canonicalize_through_the_language_type() {
        for (path, tag) in [
            ("en/index.md", "en"),
            ("zh-hant/index.md", "zh-Hant"),
            ("zh-tw/index.md", "zh-Hant"),
            ("zh/index.md", "zh-Hans"),
        ] {
            assert_eq!(declared_lang_tag(None, "index", path, None).as_deref(), Some(tag), "{path}");
        }
    }

    /// The folder-inferred rung. It is still the PAGE's evidence, so
    /// it beats the site default — a `None` here must mean the page declared
    /// nothing at all, or a site declaring a language moss cannot render chrome
    /// in would lose it at every page.
    #[test]
    fn a_folder_inferred_language_is_still_the_pages_own() {
        assert_eq!(
            declared_lang_tag(None, "post", "notes/post.md", Some(Language::ZhHant)).as_deref(),
            Some("zh-Hant")
        );
        assert_eq!(declared_lang_tag(None, "post", "notes/post.md", None), None);
    }

    /// Same priority as `resolve_document_language`'s explicit steps:
    /// frontmatter beats filename suffix beats ancestor folder.
    #[test]
    fn explicit_signals_win_in_the_documented_order() {
        assert_eq!(declared_lang_tag(Some("ko"), "post.ja", "de/post.ja.md", None), Some("ko".to_string()));
        assert_eq!(declared_lang_tag(None, "post.ja", "de/post.ja.md", None), Some("ja".to_string()));
        assert_eq!(declared_lang_tag(None, "post", "de/post.md", None), Some("de".to_string()));
    }

    /// An unrecognized `lang:` is a typo, not a fourth language. Emitting
    /// `lang="english"` would be worse than falling back to the site default.
    #[test]
    fn an_unrecognized_code_is_not_promoted_to_a_language() {
        assert_eq!(declared_lang_tag(Some("english"), "index", "index.md", None), None);
        assert_eq!(declared_lang_tag(None, "index", "english/index.md", None), None);
    }

    /// Canonical case is what avoids crawler warnings — the same reason
    /// `as_bcp47_attr` is title-cased by hand.
    #[test]
    fn emitted_tags_are_canonically_cased() {
        assert_eq!(canonical_bcp47("pt-br"), "pt-BR");
        assert_eq!(canonical_bcp47("zh-hant"), "zh-Hant");
        assert_eq!(canonical_bcp47("ja"), "ja");
        assert_eq!(declared_lang_tag(None, "index", "pt-br/index.md", None), Some("pt-BR".to_string()));
    }
}

/// The last-resort default language for a SITE build (a non-English
/// user's empty/ambiguous site defaults to their language, not `en`).
///
/// Seeded once at process start by the app (`init_app_language` calls
/// [`seed_build_default_language`]) and immutable after — a build's content
/// default must stay tied to the session, not the live UI language toggle.
/// Unseeded (unit tests) it falls back to `En`.
static BUILD_DEFAULT_LANGUAGE: std::sync::OnceLock<Language> = std::sync::OnceLock::new();

/// Seed the build-default language. First call wins; later calls are no-ops.
pub fn seed_build_default_language(lang: Language) {
    let _ = BUILD_DEFAULT_LANGUAGE.set(lang);
}

/// See [`BUILD_DEFAULT_LANGUAGE`]. `En` when never seeded.
pub fn build_default_language() -> Language {
    BUILD_DEFAULT_LANGUAGE.get().copied().unwrap_or(Language::En)
}

// ── App-shell language (crossed from `infra::app_config` at M6a) ──────────
// `infra::app_advisory` resolves its localized strings against this cell, so
// the vocabulary and the live cell live beside it in the crate. Detection
// (locale probing) stays app-side; unseeded reads default to `En`.

/// Language preference for the moss app shell UI.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AppLanguage {
    #[default]
    En,
    ZhHans,
    ZhHant,
}

impl AppLanguage {
    /// Returns the BCP-47 tag string used by the webview and TypeScript registry.
    /// Note: uses hyphen (zh-hans), not underscore — distinct from the serde snake_case disk format.
    pub fn as_locale_str(&self) -> &'static str {
        match self {
            AppLanguage::En => "en",
            AppLanguage::ZhHans => "zh-hans",
            AppLanguage::ZhHant => "zh-hant",
        }
    }

    /// Map a BCP-47 language tag to AppLanguage. Unknown tags fall back to En.
    ///
    /// Delegates to `Language::from_bcp47_lenient` — the single zh-bucketing
    /// implementation (see `tests/zh_bucketing_invariant_test.rs`). The
    /// exhaustive match makes a new `Language` variant a compile error here,
    /// keeping the two enums from drifting silently.
    pub fn from_bcp47(tag: &str) -> AppLanguage {
        match crate::i18n::Language::from_bcp47_lenient(tag) {
            crate::i18n::Language::En => AppLanguage::En,
            crate::i18n::Language::ZhHans => AppLanguage::ZhHans,
            crate::i18n::Language::ZhHant => AppLanguage::ZhHant,
        }
    }

    /// The matching site-facing `Language` (the two enums are kept in lockstep).
    pub fn to_language(&self) -> crate::i18n::Language {
        match self {
            AppLanguage::En => crate::i18n::Language::En,
            AppLanguage::ZhHans => crate::i18n::Language::ZhHans,
            AppLanguage::ZhHant => crate::i18n::Language::ZhHant,
        }
    }
}


/// Process-global app language. **Mutable** so a live language switch (no
/// restart) takes effect immediately for backend advisory / progress / `Verb`
/// strings that resolve against it. `None` until seeded, which preserves the
/// original detect-on-miss semantics exactly.
///
/// Seeded at the very top of `run()` by [`init_app_language`] — before the Tauri
/// builder — so it is set even in CLI/Deploy mode where no webview ever calls
/// `ensure_language_detected` (that path needs `app.path()`). Updated at runtime
/// by [`set_app_language`] (the live-switch command). `RwLock::new(None)` is
/// `const`, so no lazy init is needed; the write scope is a single assignment
/// that cannot panic, so the lock cannot become poisoned in practice — and if it
/// ever were, reads degrade to a fresh `detect_app_language()`.
static APP_LANGUAGE: std::sync::RwLock<Option<AppLanguage>> = std::sync::RwLock::new(None);

/// Update the process-global app language at runtime (live switch, no restart).
/// Idempotent; subsequent `app_language()` reads return `lang`.
pub fn set_app_language(lang: AppLanguage) {
    if let Ok(mut g) = APP_LANGUAGE.write() {
        *g = Some(lang);
    }
}

/// The seeded app language, if any — the app layers its detect-on-miss
/// fallback over this (`infra::app_config::app_language`).
pub fn app_language_seeded() -> Option<AppLanguage> {
    APP_LANGUAGE.read().ok().and_then(|g| *g)
}

/// The process-global app language for backend string resolution. `En` when
/// never seeded (unit tests; the app seeds at the top of `main`).
pub fn app_language() -> AppLanguage {
    app_language_seeded().unwrap_or_default()
}
