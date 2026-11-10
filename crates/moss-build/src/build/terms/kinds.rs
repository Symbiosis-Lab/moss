//! The kinds table: turning `.moss/config.toml`'s `[terms]` section into the
//! one input [`super::derive_terms`] consumes.
//!
//! Split out of `terms.rs` (sibling-file pattern) so the file that resolves
//! *what a build's term namespaces are* stays apart from the file that
//! resolves *which documents belong to them* — the two grow independently
//! (this one from config-reading concerns, the other from per-document
//! derivation passes) and neither needs the other's internals.

use moss_core::terms::{AUTHOR_NS, TAGS_NS};

pub use moss_core::terms::config::BUILTIN_DEFAULT_FIELDS;

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
    /// The kind's name in the site language, resolved once at construction.
    /// Persisted with the kind; read it through [`Self::title_in`].
    pub title: String,
    /// The config `title` as `(language tag, title)` pairs; a plain string is
    /// the `en` entry, which every language falls back to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub titles: Vec<(String, String)>,
    /// `type = "place"` in `[terms.<key>]`. The only typed kind — a plain
    /// bool, not an enum, because a place is the only kind type moss
    /// recognizes so far. Drives which kinds `build::terms::places::attach_parents`
    /// fills; render-time code does NOT gate on this field — it gates on
    /// whether `parents` or its inverse actually has an entry for the term
    /// in question, so a non-place kind renders no breadcrumb/children by
    /// simply having nothing recorded, not by an extra check.
    ///
    /// `skip_serializing_if` on the negation, not a plain `#[serde(default)]`
    /// alone: without it every kind in every article map, place or not,
    /// gains a literal `"is_place": false`, which is not byte-identical to a
    /// pre-places-slice map.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_place: bool,
    /// Generic parent hierarchy: term key (already `term_folder_key`-shaped,
    /// e.g. `places/kyoto`) → parent's DISPLAY NAME (not yet a key —
    /// resolving a display name to its own key, to keep walking upward, is
    /// `derive_terms` pass 2's job, once it already has the fully-built
    /// kinds table in scope). Only place kinds are ever filled; every other
    /// kind carries an empty map and pays nothing. Filled once, at the
    /// config stage, by `build::terms::places::attach_parents` —
    /// `derive_terms` itself never reads the gazetteer.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub parents: std::collections::BTreeMap<String, String>,
    /// `[terms.<key>] explorer`, carried through only for the place-typed
    /// kind — `term_kinds` drops a declared value for every other kind, the
    /// same way `parents` is only ever filled for one. `None` means
    /// "default on" (`explorer_enabled` resolves the default); `Some(false)`
    /// is the only way to see the interactive layer turned off. Always
    /// `None` for a non-place kind, so the key is accepted without error but
    /// has no effect there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explorer: Option<bool>,
    /// `[terms.<key>] line`, read through [`Self::draws_line`]. `None` is the
    /// default, so a kind that sets none serializes as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<bool>,
    /// Explicit slugs from `.moss/places.toml`, keyed by the slug the place's
    /// name derives. Only a place-typed kind is ever filled, by `attach_parents`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub slugs: std::collections::BTreeMap<String, String>,
    /// `[terms.<key>] cards = true`: every listing card for a page with values
    /// in this kind's fields shows those names in its meta. Read by
    /// `build::terms::roles::set_card_credits`; `false` for a value that is
    /// not a bool, with a diagnostic from `term_kinds`.
    ///
    /// `skip_serializing_if` for the same reason as `is_place`: an unset
    /// default must not add a key to every serialized kind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cards: bool,
    /// `[terms.<key>] count`, the card count wording per language tag, as
    /// `titles` is. Read through [`Self::count_in`]. Skipped when empty, so a
    /// kind that sets none serializes as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<(String, String)>,
}

impl TermKind {
    /// The card count wording for a page in `lang`, `None` when the kind sets
    /// none. `false` comes back as empty text, which hides the count.
    pub fn count_in(&self, lang: &str) -> Option<moss_core::frontmatter_union::ChildrenCount> {
        moss_core::fields::label_in(&self.counts, lang)
            .map(|text| moss_core::frontmatter_union::ChildrenCount(text.to_string()))
    }

    /// Whether the places explorer should activate for this kind: only ever
    /// true for the place-typed kind, default-on absent an explicit
    /// `explorer = false`. The one place both the injection predicate
    /// (`features::should_inject_places_explorer`) and the markup handshake
    /// (`PlaceMapRenderContext`) read this, so they cannot resolve the
    /// default differently.
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

    /// The kind's name in `edition` (`""` is the default edition), which its
    /// root page is headed and its nav entry labelled with. An edition moss
    /// has no interface strings for is dressed in the site language's, so
    /// without a label of its own it takes the site language's name.
    pub fn title_in(&self, edition: &str) -> String {
        let labelled = moss_core::fields::label_in(&self.titles, edition).is_some();
        if edition.is_empty() || (!labelled && crate::i18n::Language::from_code(edition).is_none()) {
            return self.title.clone();
        }
        kind_title(&self.key, &self.titles, edition)
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

/// A kind's name in `lang`: its config `title` for that language by the rule a
/// declared field's labels follow, else a built-in's translated title, else
/// its key capitalized.
fn kind_title(key: &str, titles: &[(String, String)], lang: &str) -> String {
    if let Some(label) = moss_core::fields::label_in(titles, lang) {
        return label.to_string();
    }
    if key == AUTHOR_NS || key == TAGS_NS {
        let lang = crate::i18n::Language::from_code(lang).unwrap_or_default();
        return crate::i18n::term_root_title(lang, key).to_string();
    }
    let mut chars = key.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The name-list fields a config declares beyond the built-in five, in
/// declaration order, deduplicated. The editor's schema adds them.
pub fn declared_name_list_fields(cfg: &crate::config::ConfigFile) -> Vec<String> {
    moss_core::terms::config::declared_name_list_fields(&cfg.terms_kinds()).0
}

/// Turn config into the kinds table [`super::derive_terms`] derives from —
/// the one unified read of `.moss/config.toml`'s `[terms]` section
/// ([`crate::config::ConfigFile::terms_kinds`]).
///
/// The name-list set for this build is the built-in five (the `name_list`
/// flag in [`moss_core::schema_fields::BUILTIN_FIELDS`]) plus every name a declared
/// `fields` list names: a declared field is a name-list field for this site.
/// A name refused by [`moss_core::terms::config::name_list_refusal`] (a built-in non-list field such as
/// `title`, or a name claiming a built-in's term page) is a diagnostic, not a
/// silent drop. A field a
/// DECLARED (non-built-in) kind claims is removed from any built-in kind
/// that would otherwise list it by default, so a field belongs to at most
/// one kind — naming `author` in `[terms.people]` takes it out of
/// `authors/`. `title` is resolved here, in the site language (see
/// `kind_title`).
/// Nothing about the result is patched later.
///
/// A declared kind that reuses a built-in's own key (`[terms.tags]`,
/// `[terms.authors]`) never reaches this function as a *separate* `RawKind`
/// sharing that key — `ConfigFile::terms_kinds` already resolved that one
/// key to one `RawKind` before this ever iterates it. `is_builtin` here is
/// keyed on `kind.key`, not on which branch produced the `RawKind`, so a
/// declared `[terms.tags]` with no explicit `title` still falls back to the
/// i18n default correctly.
pub fn term_kinds(cfg: &crate::config::ConfigFile, site_lang: &str) -> Vec<TermKind> {
    let raw_kinds = cfg.terms_kinds();
    let (filtered, warnings) = moss_core::terms::config::filter_fields(&raw_kinds);
    for warning in warnings {
        crate::build::cli_output::log_warn_problem!("{warning}");
    }

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
            let title = kind_title(&kind.key, &kind.titles, site_lang);
            if kind.counts.iter().any(|(_, text)| moss_core::frontmatter_union::ChildrenCount(text.clone()).lacks_placeholder()) {
                crate::build::cli_output::log_warn_problem!(
                    "[terms.{}] count has no {{n}}, so every card shows the same text",
                    kind.key
                );
            }
            let is_place = match kind.kind_type.as_deref() {
                Some("place") => true,
                Some(other) => {
                    crate::build::cli_output::log_warn_problem!(
                        "[terms.{}] has type = \"{}\", which moss does not recognize; ignoring",
                        kind.key,
                        other
                    );
                    false
                }
                None => false,
            };
            // Dropped for every non-place kind: `explorer` is ignored, not an
            // error, on a kind that isn't `type = "place"` (there is nothing
            // for it to turn off), the same posture `is_place` itself takes
            // toward an unrecognized `type`.
            let explorer = is_place.then_some(kind.explorer).flatten();
            TermKind { key: kind.key, fields, title, titles: kind.titles, counts: kind.counts, is_place, parents: Default::default(), explorer, line: kind.line, slugs: Default::default(), cards: kind.cards.unwrap_or(false) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_site_language_and_name_lists_match_core_bundle() {
        let root = tempfile::tempdir().unwrap();
        let config = "[site]\nlang = 'invalid'\n[terms]\nauthor = false\n[terms.zebra]\nfields = ['jury']\n[terms.people]\nfields = ['editor', 'tag', 'title']\n";
        let home = "---\nlang: ' FR '\n---\nHome";
        std::fs::write(root.path().join("index.md"), home).unwrap();
        let cfg = crate::config::ConfigFile::parse(config).unwrap();
        let lang = crate::i18n::detect::resolve_site_default_lang(
            cfg.site_lang(), Some("index.md"), &[], root.path().to_str().unwrap(),
        );
        let (bundle, _) = moss_core::page::site_bundle(moss_core::page::SiteSources {
            files: vec!["index.md".into()],
            texts: std::collections::HashMap::from([("index.md".into(), home.into())]),
            config_toml: Some(config.into()), ..Default::default()
        });
        assert_eq!(lang, "fr");
        assert_eq!(bundle.lang, lang);
        assert_eq!(declared_name_list_fields(&cfg), ["tags", "editor", "jury"]);
        assert_eq!(bundle.declared_name_lists, declared_name_list_fields(&cfg));
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
