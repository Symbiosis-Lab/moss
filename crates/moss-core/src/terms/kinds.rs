//! A site's term kinds, resolved from the `[terms]` tables config reads.
use super::config::{filter_fields, RawKind};
use super::{AUTHOR_NS, TAGS_NS};

/// One term kind: a URL namespace fed by one or more name-list schema fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TermKind {
    /// URL namespace / pseudo-folder prefix: `"authors"`, `"tags"`, `"people"`.
    pub key: String,
    /// Frontmatter field names feeding this kind, in the order their
    /// sections render (`["author", "editor", "jury"]`).
    pub fields: Vec<String>,
    /// The kind's name in the site language. Read it through [`Self::title_in`].
    pub title: String,
    /// The config `title` as `(language tag, title)` pairs; a plain string is
    /// the `en` entry, which every language falls back to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub titles: Vec<(String, String)>,
    /// `type = "place"`, the only kind type moss recognizes. Render code gates
    /// on whether `parents` has an entry for a term, not on this flag.
    ///
    /// Skipped when false so a serialized non-place kind gains no key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_place: bool,
    /// Term key (`places/kyoto`) → its parent's display name, not yet a key.
    /// Only a place kind is ever filled, from the gazetteer.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub parents: std::collections::BTreeMap<String, String>,
    /// `[terms.<key>] explorer`, kept only for a place kind. `None` means on;
    /// read it through [`Self::explorer_enabled`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explorer: Option<bool>,
    /// `[terms.<key>] line`, read through [`Self::draws_line`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<bool>,
    /// Explicit slugs from the gazetteer, keyed by the slug the place's name
    /// derives. Only a place kind is ever filled.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub slugs: std::collections::BTreeMap<String, String>,
    /// `[terms.<key>] cards = true`: listing cards show this kind's names.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cards: bool,
    /// `[terms.<key>] count` per language tag, as `titles` is. Read through
    /// [`Self::count_in`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<(String, String)>,
}

impl TermKind {
    /// The card count wording for a page in `lang`, `None` when the kind sets
    /// none. `false` comes back as empty text, which hides the count.
    pub fn count_in(&self, lang: &str) -> Option<crate::frontmatter_union::ChildrenCount> {
        crate::fields::label_in(&self.counts, lang)
            .map(|text| crate::frontmatter_union::ChildrenCount(text.to_string()))
    }

    /// Whether the places explorer activates: a place kind only, on unless
    /// `explorer = false`. Both the injection predicate and the markup read
    /// this, so they cannot resolve the default differently.
    pub fn explorer_enabled(&self) -> bool {
        self.is_place && self.explorer.unwrap_or(true)
    }

    /// Whether pages draw this kind's names as a line of links: only a
    /// place-typed kind does unless `line` says otherwise.
    pub fn draws_line(&self) -> bool {
        self.line.unwrap_or(self.is_place)
    }

    /// The fields a line is drawn for. A place-typed kind's fields all feed
    /// `location` (mirrored, then inherited by a work's companions), so its
    /// line is that one field.
    pub fn line_fields(&self) -> Vec<&str> {
        if self.is_place { vec!["location"] } else { self.fields.iter().map(String::as_str).collect() }
    }

    /// The kind's name in `edition` (`""` is the default edition). An edition
    /// moss has no interface strings for is dressed in the site language's,
    /// so without a label of its own it takes the site language's name.
    pub fn title_in(&self, edition: &str) -> String {
        let labelled = crate::fields::label_in(&self.titles, edition).is_some();
        if edition.is_empty() || (!labelled && crate::i18n::Language::from_code(edition).is_none()) {
            return self.title.clone();
        }
        kind_title(&self.key, &self.titles, edition)
    }

    /// The key of the term `name` as a page of `edition` names it
    /// (`de/partners/ada`). A place-typed kind is site-wide, since its map and
    /// gazetteer are keyed by the plain key, so it ignores the edition.
    pub fn key_in(&self, edition: &str, name: &str) -> String {
        scope(if self.is_place { "" } else { edition }, &slugged_key(&self.slugs, &self.key, name))
    }

    /// A built-in kind as config would make it, for a map written before
    /// kinds were recorded.
    pub fn builtin(key: &str, field: &str, site_lang: &str) -> TermKind {
        TermKind {
            key: key.to_string(),
            fields: vec![field.to_string()],
            title: kind_title(key, &[], site_lang),
            titles: Vec::new(),
            counts: Vec::new(),
            is_place: false,
            parents: Default::default(),
            explorer: None,
            line: None,
            slugs: Default::default(),
            cards: false,
        }
    }
}

/// A key inside an edition's tree: `de` + `partners/ada` is `de/partners/ada`.
/// The default edition (`""`) keeps the plain key.
pub fn scope(edition: &str, key: &str) -> String {
    if edition.is_empty() { key.to_string() } else { format!("{edition}/{key}") }
}

/// The key of the place `name` in `ns`, honouring an explicit slug from
/// `slugs` (keyed by the slug the name derives, so case variants of one name
/// share it).
pub fn slugged_key(slugs: &std::collections::BTreeMap<String, String>, ns: &str, name: &str) -> String {
    let derived = super::term_slug(name);
    format!("{ns}/{}", slugs.get(&derived).unwrap_or(&derived))
}

/// A kind's name in `lang`: its config `title` for that language, else a
/// built-in's translated title, else its key as a label.
fn kind_title(key: &str, titles: &[(String, String)], lang: &str) -> String {
    if let Some(label) = crate::fields::label_in(titles, lang) {
        return label.to_string();
    }
    if key == AUTHOR_NS || key == TAGS_NS {
        let lang = crate::i18n::Language::from_code(lang).unwrap_or_default();
        return crate::i18n::term_root_title(lang, key).to_string();
    }
    crate::fields::name_label(key)
}

/// The kinds table term derivation reads, with the problems found on the way.
///
/// A name [`super::config::name_list_refusal`] refuses is dropped with a
/// warning. A field a declared kind claims leaves the built-in kind that
/// would list it by default, so a field belongs to at most one kind: naming
/// `author` in `[terms.people]` takes it out of `authors/`. `title` is
/// resolved here, in `site_lang`. `parents` and `slugs` start empty; the
/// gazetteer fills them.
pub fn resolve_kinds(raw_kinds: &[RawKind], site_lang: &str) -> (Vec<TermKind>, Vec<String>) {
    let (filtered, mut warnings) = filter_fields(raw_kinds);
    let is_builtin = |key: &str| key == AUTHOR_NS || key == TAGS_NS;
    let claimed_by_a_declared_kind: std::collections::HashSet<String> = filtered
        .iter()
        .filter(|(kind, _)| !is_builtin(&kind.key))
        .flat_map(|(_, fields)| fields.iter().cloned())
        .collect();
    let kinds = filtered
        .into_iter()
        .map(|(kind, fields)| {
            let fields = if is_builtin(&kind.key) {
                fields.into_iter().filter(|f| !claimed_by_a_declared_kind.contains(f)).collect()
            } else {
                fields
            };
            let title = kind_title(&kind.key, &kind.titles, site_lang);
            if kind.counts.iter().any(|(_, text)| crate::frontmatter_union::ChildrenCount(text.clone()).lacks_placeholder()) {
                warnings.push(format!("[terms.{}] count has no {{n}}, so every card shows the same text", kind.key));
            }
            let is_place = match kind.kind_type.as_deref() {
                Some("place") => true,
                Some(other) => {
                    warnings.push(format!(
                        "[terms.{}] has type = \"{other}\", which moss does not recognize; ignoring",
                        kind.key
                    ));
                    false
                }
                None => false,
            };
            // Ignored, not an error, on a kind with nothing for it to turn off.
            let explorer = is_place.then_some(kind.explorer).flatten();
            TermKind { key: kind.key, fields, title, titles: kind.titles, counts: kind.counts, is_place, parents: Default::default(), explorer, line: kind.line, slugs: Default::default(), cards: kind.cards.unwrap_or(false) }
        })
        .collect();
    (kinds, warnings)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_kind_and_a_role_with_no_title_read_their_name_with_spaces() {
        assert_eq!(super::kind_title("angabe_ort", &[], "de"), "Angabe ort");
        assert_eq!(super::super::lines::term_role_label(crate::i18n::Language::De, "angabe_ort"), "Angabe ort");
    }
}
