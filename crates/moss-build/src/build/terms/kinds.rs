//! The kinds table: turning `.moss/config.toml`'s `[terms]` section into the
//! one input [`super::derive_terms`] consumes.
//!
//! Split out of `terms.rs` (sibling-file pattern) so the file that resolves
//! *what a build's term namespaces are* stays apart from the file that
//! resolves *which documents belong to them* — the two grow independently
//! (this one from config-reading concerns, the other from per-document
//! derivation passes) and neither needs the other's internals.

pub use moss_core::terms::config::BUILTIN_DEFAULT_FIELDS;
pub use moss_core::terms::kinds::TermKind;

/// The name-list fields a config declares beyond the built-in five, in
/// declaration order, deduplicated. The editor's schema adds them.
pub fn declared_name_list_fields(cfg: &crate::config::ConfigFile) -> Vec<String> {
    moss_core::terms::config::declared_name_list_fields(&cfg.terms_kinds()).0
}

/// The kinds table [`super::derive_terms`] derives from, resolved in core
/// ([`moss_core::terms::kinds::resolve_kinds`]); its problems are logged.
pub fn term_kinds(cfg: &crate::config::ConfigFile, site_lang: &str) -> Vec<TermKind> {
    let (kinds, warnings) = moss_core::terms::kinds::resolve_kinds(&cfg.terms_kinds(), site_lang);
    for warning in warnings {
        crate::build::cli_output::log_warn_problem!("{warning}");
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::terms::{AUTHOR_NS, TAGS_NS};

    #[test]
    fn fixture_site_language_and_name_lists_match_core_bundle() {
        let root = tempfile::tempdir().unwrap();
        let config = "[site]\nlang = 'invalid'\n[terms]\nauthor = false\n[terms.zebra]\nfields = ['jury']\n[terms.people]\nfields = ['editor', 'tag', 'title']\n";
        let home = "---\nlang: ' FR '\n---\nHome";
        std::fs::write(root.path().join("index.md"), home).unwrap();
        let cfg = crate::config::ConfigFile::parse(config).unwrap();
        let lang = crate::i18n::detect::resolve_site_lang(
            cfg.site_lang(), Some("index.md"), &[], root.path().to_str().unwrap(),
        ).code;
        let (bundle, _) = moss_core::page::site_bundle(moss_core::page::SiteSources {
            files: vec!["index.md".into()],
            texts: std::collections::HashMap::from([("index.md".into(), home.into())]),
            config_toml: Some(config.into()), ..Default::default()
        });
        assert_eq!(lang, "fr");
        assert_eq!(bundle.lang, lang);
        assert_eq!(declared_name_list_fields(&cfg), ["tags", "editor", "jury"]);
        assert_eq!(bundle.declared_name_lists, declared_name_list_fields(&cfg));
        assert_eq!(bundle.terms, term_kinds(&cfg, &lang));
    }

    #[test]
    fn declared_kind_moves_a_field_out_of_its_built_in_kind() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"author\", \"editor\"]\n")
                .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        assert!(authors.fields.is_empty(), "author moved into people: {:?}", authors.fields);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(people.fields, vec!["author".to_string(), "editor".to_string()]);
    }

    #[test]
    fn declared_place_kind_is_marked_is_place() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(places.is_place);
    }

    #[test]
    fn explorer_defaults_on_for_a_place_kind_with_no_explorer_key() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(places.explorer_enabled(), "absent key defaults the explorer on");
    }

    #[test]
    fn explorer_false_turns_the_explorer_off_for_a_place_kind() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\nexplorer = false\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(!places.explorer_enabled());
    }

    #[test]
    fn line_defaults_on_for_a_place_kind_only_and_either_way_can_be_set() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n\n[terms.venues]\ntype = \"place\"\nfields = [\"venue\"]\nline = false\n\n[terms.people]\nfields = [\"author\"]\n\n[terms.topics]\nfields = [\"topic\"]\nline = true\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let draws = |key: &str| kinds.iter().find(|k| k.key == key).unwrap().draws_line();
        assert!(draws("places"));
        assert!(!draws("venues"));
        assert!(!draws("people"));
        assert!(draws("topics"));
    }

    #[test]
    fn explorer_on_a_non_place_kind_is_ignored_not_an_error() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.people]\nfields = [\"author\"]\nexplorer = false\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(people.explorer, None, "the key is dropped for a non-place kind");
        assert!(!people.explorer_enabled(), "never true off is_place, whatever the raw value was");
    }

    #[test]
    fn unknown_kind_type_is_a_diagnostic_not_a_crash() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"country\"\nfields = [\"location\"]\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, "en");
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(!places.is_place, "an unrecognized type is ignored, not a panic");
    }

    #[test]
    fn cards_true_turns_the_kind_on_and_a_non_bool_is_a_diagnostic() {
        let on = crate::config::ConfigFile::parse("[terms.performers]\nfields = [\"conductor\"]\ncards = true\n")
            .unwrap();
        let kinds = term_kinds(&on, "en");
        assert!(kinds.iter().find(|k| k.key == "performers").unwrap().cards);
        assert!(on.warnings().is_empty(), "a bool is not a problem");

        let bad = crate::config::ConfigFile::parse("[terms.performers]\nfields = [\"conductor\"]\ncards = \"yes\"\n")
            .unwrap();
        let kinds = term_kinds(&bad, "en");
        assert!(!kinds.iter().find(|k| k.key == "performers").unwrap().cards, "a non-bool is off");
        assert_eq!(bad.warnings(), ["[terms.performers] cards must be true or false; ignoring it"]);
    }

    #[test]
    fn a_kind_with_no_type_key_is_never_a_place_kind() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"author\"]\n").unwrap();
        let kinds = term_kinds(&cfg, "en");
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert!(!people.is_place);
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
        let kinds = term_kinds(&crate::config::ConfigFile::empty(), "en");
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
        assert_eq!(authors.fields, vec!["author".to_string()]);
        assert_eq!(tags.fields, vec!["tags".to_string()]);
    }
}
