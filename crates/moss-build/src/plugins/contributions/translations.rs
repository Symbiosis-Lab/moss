//! `translations`: a manifest's own words in other languages, keyed by the
//! English text the way gettext keys a catalog by its source strings.

use std::collections::BTreeMap;

pub type Translations = BTreeMap<String, BTreeMap<String, String>>;

/// Refuse a locale moss could never match, and bound every string: a
/// translation lands in moss's own form exactly as the English it replaces.
pub(crate) fn validated(table: Translations) -> Result<Translations, String> {
    use moss_core::untrusted_text::{bounded, MAX_SENTENCE};
    table
        .into_iter()
        .map(|(locale, entries)| {
            let shaped = (2..=8).contains(&locale.len())
                && locale.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
            if !shaped {
                return Err(format!("translations: `{locale}` is not a locale such as \"zh\" or \"pt-br\""));
            }
            let entries = entries.into_iter().map(|(en, text)| (en, bounded(&text, MAX_SENTENCE))).collect();
            Ok((locale, entries))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::plugins::types::PluginManifest;
    use serde_json::json;

    fn parse(translations: serde_json::Value) -> Result<PluginManifest, String> {
        PluginManifest::parse(&json!({"name":"words","version":"1.0.0","entry":"main.js","translations":translations}).to_string())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn a_locale_moss_could_never_match_is_refused() {
        let m = parse(json!({"zh":{"Time zone":"时区"}})).unwrap();
        assert_eq!(m.translations["zh"]["Time zone"], "时区");
        assert!(parse(json!({"Chinese":{"a":"b"}})).is_err());
        assert!(parse(json!({"":{"a":"b"}})).is_err());
    }

    #[test]
    fn a_translation_cannot_reverse_the_sentence_around_it() {
        let m = parse(json!({"ar":{"Name":"\u{202e}eman"}})).unwrap();
        assert!(!m.translations["ar"]["Name"].contains('\u{202e}'));
    }
}
