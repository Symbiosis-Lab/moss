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
    /// `[terms.<key>] line`, carried through only for the place-typed kind,
    /// like `explorer`. `Some(false)` is the only way to drop the automatic
    /// place line; `None` means the default, on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<bool>,
    /// `[terms.<key>] cards = true`: every listing card for a page with values
    /// in this kind's fields shows those names in its meta. Read by
    /// `build::terms::roles::set_card_credits`; `false` for a value that is
    /// not a bool, with a diagnostic from `term_kinds`.
    ///
    /// `skip_serializing_if` for the same reason as `is_place`: an unset
    /// default must not add a key to every serialized kind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cards: bool,
}

impl TermKind {
    /// Whether the places explorer should activate for this kind: only ever
    /// true for the place-typed kind, default-on absent an explicit
    /// `explorer = false`. The one place both the injection predicate
    /// (`features::should_inject_places_explorer`) and the markup handshake
    /// (`PlaceMapRenderContext`) read this, so they cannot resolve the
    /// default differently.
    pub fn explorer_enabled(&self) -> bool {
        self.is_place && self.explorer.unwrap_or(true)
    }
}

/// Why a name in a `[terms.<key>] fields` list cannot be a name-list field, or
/// `None` when it can. A built-in field that is not a name list is refused,
/// since reading its value as a list of names would break it. So is a name
/// whose claim key (`<name>_page`) is a built-in's claim key: `tag` would claim
/// `tag_page:`, which `tags` already owns.
pub fn name_list_refusal(field: &str) -> Option<String> {
    match moss_core::schema_fields::BUILTIN_FIELDS.iter().find(|b| b.name == field) {
        Some(b) if !b.name_list => Some("a built-in field that is not a name list".to_string()),
        Some(_) => None,
        None => {
            let claim = super::claim_field_key(field);
            moss_core::schema_fields::term_claim_fields()
                .any(|c| c == claim)
                .then(|| format!("its claim key {claim} belongs to a built-in field"))
        }
    }
}

/// The name-list fields a config declares beyond the built-in five, in
/// declaration order, deduplicated. The editor's schema adds them.
pub fn declared_name_list_fields(cfg: &crate::config::ConfigFile) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for kind in cfg.terms_kinds() {
        for f in kind.fields {
            if name_list_refusal(&f).is_none() && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// Turn config into the kinds table [`super::derive_terms`] derives from —
/// the one unified read of `.moss/config.toml`'s `[terms]` section
/// ([`crate::config::ConfigFile::terms_kinds`]).
///
/// The name-list set for this build is the built-in five (the `name_list`
/// flag in [`moss_core::schema_fields::BUILTIN_FIELDS`]) plus every name a declared
/// `fields` list names: a declared field is a name-list field for this site.
/// A name refused by [`name_list_refusal`] (a built-in non-list field such as
/// `title`, or a name claiming a built-in's term page) is a diagnostic, not a
/// silent drop. A field a
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
    let filtered: Vec<(crate::config::RawKind, Vec<String>)> = cfg
        .terms_kinds()
        .into_iter()
        .map(|kind| {
            let fields = kind
                .fields
                .iter()
                .filter(|f| {
                    match name_list_refusal(f) {
                        None => true,
                        Some(reason) => {
                            crate::build::cli_output::log_warn_problem!(
                                "[terms.{}] names field \"{}\", which cannot be a name-list field ({}); ignoring",
                                kind.key,
                                f,
                                reason
                            );
                            false
                        }
                    }
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
            let line = is_place.then_some(kind.line).flatten();
            let cards = match kind.cards {
                None => false,
                Some(toml::Value::Boolean(on)) => on,
                Some(other) => {
                    crate::build::cli_output::log_warn_problem!(
                        "[terms.{}] cards must be true or false, not {}; ignoring",
                        kind.key,
                        other
                    );
                    false
                }
            };
            TermKind { key: kind.key, fields, title, is_place, parents: Default::default(), explorer, line, cards }
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
    fn a_declared_field_is_a_name_list_field_for_the_site() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"editor\", \"ghost\"]\n")
                .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(
            people.fields,
            vec!["editor".to_string(), "ghost".to_string()],
            "a name no built-in knows is still a name-list field once declared"
        );
    }

    #[test]
    fn a_built_in_field_that_is_not_a_name_list_is_refused() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"editor\", \"title\"]\n")
                .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert_eq!(people.fields, vec!["editor".to_string()], "title is not a name list");
    }

    #[test]
    fn a_declared_name_whose_claim_key_is_a_built_ins_is_refused() {
        // `tag` would claim `tag_page:`, the key `tags` already owns.
        let cfg = crate::config::ConfigFile::parse("[terms.people]\nfields = [\"tag\", \"place\"]\n")
            .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
        assert!(people.fields.is_empty(), "tag and place collide with tags and location: {:?}", people.fields);
    }

    #[test]
    fn declared_place_kind_is_marked_is_place() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(places.is_place);
    }

    #[test]
    fn explorer_defaults_on_for_a_place_kind_with_no_explorer_key() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(places.explorer_enabled(), "absent key defaults the explorer on");
    }

    #[test]
    fn explorer_false_turns_the_explorer_off_for_a_place_kind() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\nexplorer = false\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(!places.explorer_enabled());
    }

    #[test]
    fn line_false_is_kept_for_a_place_kind_and_dropped_for_any_other() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\nline = false\n\n[terms.people]\nfields = [\"author\"]\nline = false\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        assert_eq!(kinds.iter().find(|k| k.key == "places").unwrap().line, Some(false));
        assert_eq!(kinds.iter().find(|k| k.key == "people").unwrap().line, None);
    }

    #[test]
    fn explorer_on_a_non_place_kind_is_ignored_not_an_error() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.people]\nfields = [\"author\"]\nexplorer = false\n",
        )
        .unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
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
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
        assert!(!places.is_place, "an unrecognized type is ignored, not a panic");
    }

    #[test]
    fn cards_true_turns_the_kind_on_and_a_non_bool_is_a_diagnostic() {
        let _ = crate::build::cli_output::take_cli_problems();
        let on = crate::config::ConfigFile::parse("[terms.performers]\nfields = [\"conductor\"]\ncards = true\n")
            .unwrap();
        let kinds = term_kinds(&on, crate::i18n::Language::En);
        assert!(kinds.iter().find(|k| k.key == "performers").unwrap().cards);
        assert_eq!(crate::build::cli_output::take_cli_problems(), 0, "a bool is not a problem");

        let bad = crate::config::ConfigFile::parse("[terms.performers]\nfields = [\"conductor\"]\ncards = \"yes\"\n")
            .unwrap();
        let kinds = term_kinds(&bad, crate::i18n::Language::En);
        assert!(!kinds.iter().find(|k| k.key == "performers").unwrap().cards, "a non-bool is off");
        assert_eq!(crate::build::cli_output::take_cli_problems(), 1, "a non-bool is one diagnostic");
    }

    #[test]
    fn a_kind_with_no_type_key_is_never_a_place_kind() {
        let cfg =
            crate::config::ConfigFile::parse("[terms.people]\nfields = [\"author\"]\n").unwrap();
        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
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
        let kinds = term_kinds(&crate::config::ConfigFile::empty(), crate::i18n::Language::En);
        let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
        let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
        assert_eq!(authors.fields, vec!["author".to_string()]);
        assert_eq!(tags.fields, vec!["tags".to_string()]);
    }
}
