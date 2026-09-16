//! Filename language suffix parsing
//!
//! Parses language suffixes from filenames like `my-post.zh-hans.md`.

use super::Language;

/// Result of parsing a filename for language suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFilename {
    /// The base stem without language suffix (e.g., "my-post" from "my-post.zh-hans.md")
    pub stem: String,
    /// The detected language from the suffix, if any
    pub lang: Option<Language>,
}

/// Parse a filename stem (without final extension) for a language suffix.
///
/// Given a stem like "my-post.zh-hans" (from "my-post.zh-hans.md"),
/// extracts the base name and language.
///
/// # Examples
/// - "my-post.zh-hans" -> stem="my-post", lang=Some(ZhHans)
/// - "my-post" -> stem="my-post", lang=None
/// - "about.en" -> stem="about", lang=Some(En)
pub fn parse_filename_stem(stem: &str) -> ParsedFilename {
    // Find the last dot to check for a language suffix
    if let Some((base, suffix)) = stem.rsplit_once('.') {
        if let Some(lang) = Language::from_code(suffix) {
            return ParsedFilename {
                stem: base.to_string(),
                lang: Some(lang),
            };
        }
    }

    ParsedFilename {
        stem: stem.to_string(),
        lang: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_suffix() {
        let result = parse_filename_stem("my-post");
        assert_eq!(result.stem, "my-post");
        assert_eq!(result.lang, None);
    }

    #[test]
    fn test_english_suffix() {
        let result = parse_filename_stem("about.en");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::En));
    }

    #[test]
    fn test_chinese_simplified_suffix() {
        let result = parse_filename_stem("my-trip.zh-hans");
        assert_eq!(result.stem, "my-trip");
        assert_eq!(result.lang, Some(Language::ZhHans));
    }

    #[test]
    fn test_chinese_traditional_suffix() {
        let result = parse_filename_stem("my-trip.zh-hant");
        assert_eq!(result.stem, "my-trip");
        assert_eq!(result.lang, Some(Language::ZhHant));
    }

    #[test]
    fn test_zh_cn_alias() {
        let result = parse_filename_stem("post.zh-cn");
        assert_eq!(result.stem, "post");
        assert_eq!(result.lang, Some(Language::ZhHans));
    }

    #[test]
    fn test_zh_tw_alias() {
        let result = parse_filename_stem("post.zh-tw");
        assert_eq!(result.stem, "post");
        assert_eq!(result.lang, Some(Language::ZhHant));
    }

    #[test]
    fn test_case_insensitive() {
        let result = parse_filename_stem("about.EN");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::En));
    }

    #[test]
    fn test_non_language_dot_suffix_preserved() {
        // "2025-01-15" has dots that aren't language codes
        let result = parse_filename_stem("file.v2");
        assert_eq!(result.stem, "file.v2");
        assert_eq!(result.lang, None);
    }

    #[test]
    fn test_multiple_dots_only_last_checked() {
        let result = parse_filename_stem("my.fancy.post.zh-hans");
        assert_eq!(result.stem, "my.fancy.post");
        assert_eq!(result.lang, Some(Language::ZhHans));
    }

    #[test]
    fn test_empty_string() {
        let result = parse_filename_stem("");
        assert_eq!(result.stem, "");
        assert_eq!(result.lang, None);
    }

    // --- Task 2.6: `zh` as shorthand for `zh-hans` ---

    #[test]
    fn test_zh_shorthand_maps_to_zh_hans() {
        // `.zh` should be accepted as a shorthand for `.zh-hans`
        let result = parse_filename_stem("about.zh");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::ZhHans));
    }

    #[test]
    fn test_zh_shorthand_case_insensitive() {
        let result = parse_filename_stem("about.ZH");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::ZhHans));
    }

    #[test]
    fn test_zh_hant_remains_distinct_from_zh() {
        // zh-hant should still resolve to ZhHant, not collapse to ZhHans
        let result = parse_filename_stem("about.zh-hant");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::ZhHant));
    }

    #[test]
    fn test_zh_tw_remains_distinct_from_zh() {
        let result = parse_filename_stem("about.zh-tw");
        assert_eq!(result.stem, "about");
        assert_eq!(result.lang, Some(Language::ZhHant));
    }
}
