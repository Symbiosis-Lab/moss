//! Matching a table's country cell to a shape.

use super::shapes::{self, Shapes};
use std::collections::HashMap;
use std::sync::OnceLock;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Names an author is likely to write that Natural Earth spells differently,
/// each with the alpha-3 code of the country it means.
const ALIASES: &[(&str, &str)] = &[
    ("uk", "GBR"),
    ("great britain", "GBR"),
    ("usa", "USA"),
    ("us", "USA"),
    ("uae", "ARE"),
    ("ivory coast", "CIV"),
    ("turkey", "TUR"),
    ("turkiye", "TUR"),
    ("czech republic", "CZE"),
    ("burma", "MMR"),
    ("swaziland", "SWZ"),
    ("macedonia", "MKD"),
    ("cape verde", "CPV"),
    ("east timor", "TLS"),
    ("vatican", "VAT"),
];

/// What a country cell means.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Matched {
    /// Index into [`Shapes::countries`].
    Country(usize),
    /// Land Natural Earth marks as disputed or breakaway throughout.
    Disputed(usize),
    Antarctica,
    Unmatched,
}

/// A name or code compared without case, accents or punctuation.
fn fold(text: &str) -> String {
    let plain: String = text.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase();
    plain.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// A level or column header compared the way a country name is.
pub(crate) fn same(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// The URL-fragment form of a country's name.
pub(crate) fn slug(name: &str) -> String {
    fold(name).replace(' ', "-")
}

/// The embedded shapes and the names that reach them.
pub(crate) struct Atlas {
    pub shapes: &'static Shapes,
    by_name: HashMap<String, usize>,
}

impl Atlas {
    pub(crate) fn get() -> &'static Atlas {
        static ATLAS: OnceLock<Atlas> = OnceLock::new();
        ATLAS.get_or_init(|| {
            let shapes = shapes::embedded();
            let mut by_name = HashMap::new();
            for (i, c) in shapes.countries.iter().enumerate() {
                for key in [&c.code, &c.adm0, &c.name, &c.name_long, &c.admin, &c.formal] {
                    if !key.is_empty() {
                        by_name.entry(fold(key)).or_insert(i);
                    }
                }
            }
            for (alias, code) in ALIASES {
                if let Some(i) = shapes.countries.iter().position(|c| c.code == *code) {
                    by_name.insert((*alias).to_string(), i);
                }
            }
            Atlas { shapes, by_name }
        })
    }

    pub(crate) fn find(&self, cell: &str) -> Matched {
        let key = fold(cell);
        if key == "antarctica" || key == "ata" {
            return Matched::Antarctica;
        }
        match self.by_name.get(&key) {
            Some(&i) if self.shapes.countries[i].disputed_name => Matched::Disputed(i),
            Some(&i) => Matched::Country(i),
            None => Matched::Unmatched,
        }
    }

    /// The short names an author is likely to write for the country (UK, USA).
    pub(crate) fn aliases_of(&self, country: usize) -> impl Iterator<Item = &'static str> {
        let code = &self.shapes.countries[country].code;
        ALIASES.iter().filter(move |(_, c)| c == code).map(|(alias, _)| *alias)
    }

    /// The country's URL fragment, from its full name.
    pub(crate) fn slug_of(&self, country: usize) -> String {
        slug(&self.shapes.countries[country].name_long)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(cell: &str) -> Option<String> {
        let atlas = Atlas::get();
        match atlas.find(cell) {
            Matched::Country(i) => Some(atlas.shapes.countries[i].code.clone()),
            _ => None,
        }
    }

    #[test]
    fn matches_names_codes_and_forms_ignoring_case_and_accents() {
        assert_eq!(code("mexico").as_deref(), Some("MEX"));
        assert_eq!(code("MEX").as_deref(), Some("MEX"));
        assert_eq!(code("  Côte d'Ivoire ").as_deref(), Some("CIV"));
        assert_eq!(code("Cote d’Ivoire").as_deref(), Some("CIV"));
        assert_eq!(code("Cafe\u{0301}").as_deref(), None);
        assert_eq!(code("Federal Republic of Germany").as_deref(), Some("DEU"), "formal names match too");
        assert_eq!(code("Russian Federation").as_deref(), Some("RUS"));
        assert_eq!(code("France").as_deref(), Some("FRA"));
        assert_eq!(code("Norway").as_deref(), Some("NOR"));
    }

    #[test]
    fn every_alias_reaches_its_country() {
        let want = [
            ("UK", "GBR"), ("USA", "USA"), ("UAE", "ARE"), ("Ivory Coast", "CIV"), ("Turkey", "TUR"),
            ("Czech Republic", "CZE"), ("Burma", "MMR"), ("Swaziland", "SWZ"), ("Macedonia", "MKD"),
            ("Cape Verde", "CPV"), ("East Timor", "TLS"), ("Vatican", "VAT"),
        ];
        for (alias, expected) in want {
            assert_eq!(code(alias).as_deref(), Some(expected), "{alias}");
        }
    }

    #[test]
    fn disputed_antarctic_and_unknown_names_are_told_apart() {
        let atlas = Atlas::get();
        assert!(matches!(atlas.find("Somaliland"), Matched::Disputed(_)));
        assert!(matches!(atlas.find("N. Cyprus"), Matched::Disputed(_)));
        assert_eq!(atlas.find("Antarctica"), Matched::Antarctica);
        assert_eq!(atlas.find("Mexco"), Matched::Unmatched);
    }

    #[test]
    fn slugs_are_url_fragments() {
        assert_eq!(slug("Côte d'Ivoire"), "cote-d-ivoire");
        assert_eq!(slug("United States of America"), "united-states-of-america");
        assert!(same("  Côte d'Ivoire", "cote d’ivoire"));
    }
}
