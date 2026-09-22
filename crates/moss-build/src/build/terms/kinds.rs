//! The kinds table: turning `.moss/config.toml`'s `[terms]` section into the
//! one input [`super::derive_terms`] consumes.
//!
//! Split out of `terms.rs` (sibling-file pattern) so the file that resolves
//! *what a build's term namespaces are* stays apart from the file that
//! resolves *which documents belong to them* — the two grow independently
//! (this one from config-reading concerns, the other from per-document
//! derivation passes) and neither needs the other's internals.

use moss_core::terms::{AUTHOR_NS, TAGS_NS};

/// The two built-in namespaces and the field each one carries by default.
/// One table, because two readers have to agree about it: config builds the
/// built-in kinds from it, and [`super::kind_move_stubs`] decides a field
/// has MOVED by comparing the kinds table against it.
pub const BUILTIN_DEFAULT_FIELDS: [(&str, &str); 2] = [(AUTHOR_NS, "author"), (TAGS_NS, "tags")];

/// One term kind: a URL namespace fed by one or more name-list schema
/// fields. Built once per build from `.moss/config.toml`. Replaces
/// `AUTHOR_NS`/`TAGS_NS` as the derivation loop's only input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TermKind {
    /// URL namespace / pseudo-folder prefix: `"authors"`, `"tags"`, `"people"`.
    pub key: String,
    /// Frontmatter field names feeding this kind, in the order their
    /// sections render (`["author", "editor", "jury"]`).
    pub fields: Vec<String>,
    /// Namespace-root heading, resolved once at construction and never read
    /// with a fallback: a declared kind's config `title` or its own key; a
    /// built-in's `i18n::term_root_title`. There is nothing to fall back to.
    pub title: String,
}

/// Turn config into the kinds table [`super::derive_terms`] derives from —
/// the one unified read of `.moss/config.toml`'s `[terms]` section
/// ([`crate::config::ConfigFile::terms_kinds`]).
///
/// Every declared `fields` list is filtered through
/// [`moss_core::schema_fields::name_list_fields`], dropping an unrecognized
/// name with a diagnostic rather than silently ignoring it. A field a
/// DECLARED (non-built-in) kind claims is removed from any built-in kind
/// that would otherwise list it by default, so a field belongs to at most
/// one kind — naming `author` in `[terms.people]` takes it out of
/// `authors/`. `title` is resolved fully here: a declared kind's own
/// `title`, or its key; a built-in's `i18n::term_root_title(lang, key)`.
/// Nothing about the result is patched later.
///
/// A declared kind that reuses a built-in's own key (`[terms.tags]`,
/// `[terms.authors]`) never reaches this function as a *separate* `RawKind`
/// sharing that key — `ConfigFile::terms_kinds` already resolved that one
/// key to one `RawKind` before this ever iterates it. `is_builtin` here is
/// keyed on `kind.key`, not on which branch produced the `RawKind`, so a
/// declared `[terms.tags]` with no explicit `title` still falls back to the
/// i18n default correctly.
pub fn term_kinds(cfg: &crate::config::ConfigFile, lang: crate::i18n::Language) -> Vec<TermKind> {
    let recognized: std::collections::HashSet<&str> =
        moss_core::schema_fields::name_list_fields().collect();

    let filtered: Vec<(crate::config::RawKind, Vec<String>)> = cfg
        .terms_kinds()
        .into_iter()
        .map(|kind| {
            let fields = kind
                .fields
                .iter()
                .filter(|f| {
                    let ok = recognized.contains(f.as_str());
                    if !ok {
                        crate::build::cli_output::log_warn_problem!(
                            "[terms.{}] names field \"{}\", which is not a name-list field; ignoring",
                            kind.key,
                            f
                        );
                    }
                    ok
                })
                .cloned()
                .collect();
            (kind, fields)
        })
        .collect();

    let claimed_by_a_declared_kind: std::collections::HashSet<String> = filtered
        .iter()
        .filter(|(kind, _)| kind.key != AUTHOR_NS && kind.key != TAGS_NS)
        .flat_map(|(_, fields)| fields.iter().cloned())
        .collect();

    filtered
        .into_iter()
        .map(|(kind, fields)| {
            let is_builtin = kind.key == AUTHOR_NS || kind.key == TAGS_NS;
            let fields = if is_builtin {
                fields.into_iter().filter(|f| !claimed_by_a_declared_kind.contains(f)).collect()
            } else {
                fields
            };
            let title = kind.title.unwrap_or_else(|| {
                if is_builtin {
                    crate::i18n::term_root_title(lang, &kind.key).to_string()
                } else {
                    kind.key.clone()
                }
            });
            TermKind { key: kind.key, fields, title }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_kind_off_by_config_derives_nothing() {
        let cfg = crate::config::ConfigFile::parse("[terms]\ntags = false\n").unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
        assert!(tags.fields.is_empty(), "tags = false empties the built-in kind: {:?}", tags.fields);
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        assert_eq!(authors.fields, vec!["author".to_string()], "authors kind untouched");
    }

    #[test]
    fn declared_kind_moves_a_field_out_of_its_built_in_kind() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"author\", \"editor\"]\n")
                .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        assert!(authors.fields.is_empty(), "author moved into people: {:?}", authors.fields);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(people.fields, vec!["author".to_string(), "editor".to_string()]);
    }

    #[test]
    fn unknown_declared_field_is_a_diagnostic_not_a_crash() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"editor\", \"ghost\"]\n")
                .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(
            people.fields,
            vec!["editor".to_string()],
            "an unrecognized field name is dropped with a diagnostic, not a panic"
        );
    }

    /// A vault with no `.moss/config.toml` at all still yields both
    /// built-in kinds, each carrying its own default field — the same
    /// expression `SiteConfig`'s construction site (`build/pipeline.rs`)
    /// evaluates when `cfg` is `None`, at `Language::En` (what a
    /// config-less vault's `site_lang` resolves to absent any other
    /// signal). That wiring itself is proven by the snapshot suite: A0's
    /// two witness fixtures declare no `[terms]` config, and ablating the
    /// `pipeline.rs` construction line turns both of them red.
    #[test]
    fn absent_config_still_yields_the_two_built_in_kinds() {
        let kinds = term_kinds(&crate::config::ConfigFile::empty(), crate::i18n::Language::En);
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
        assert_eq!(authors.fields, vec!["author".to_string()]);
        assert_eq!(tags.fields, vec!["tags".to_string()]);
    }
}
