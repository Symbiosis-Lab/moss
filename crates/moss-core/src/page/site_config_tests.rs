use super::*;

#[test]
fn typed_config_accepts_inline_dotted_sections_and_plugin_arrays() {
    for text in [
        "site = { lang = 'de' }\nterms = { author = false, tags = true, places = { fields = ['location'], title = 'Places', count = '{n} places', type = 'place', explorer = false, line = true, cards = true, nav = true, weight = -2 } }",
        "site.lang = 'de'\nterms.author = false\nterms.tags = true\nterms.places.fields = ['location']\nterms.places.title = 'Places'\nterms.places.count = '{n} places'\nterms.places.type = 'place'\nterms.places.explorer = false\nterms.places.line = true\nterms.places.cards = true\nterms.places.nav = true\nterms.places.weight = -2",
        "[site]\nlang = 'de'\n[terms]\nauthor = false\ntags = true\n[terms.places]\nfields = ['location']\ntitle = 'Places'\ncount = '{n} places'\ntype = 'place'\nexplorer = false\nline = true\ncards = true\nnav = true\nweight = -2\n[[plugins]]\nname = 'first'\noptions = { enabled = true }\n[[plugins]]\nname = 'second'",
    ] {
        let (config, warnings) = parse_config(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.site_lang(), Some("de"));
        let kinds = config.terms_kinds();
        assert_eq!(kinds.len(), 3);
        assert!(kinds[0].fields.is_empty());
        assert_eq!(kinds[1].fields, ["tags"]);
        let places = &kinds[2];
        assert_eq!(places.fields, ["location"]);
        assert_eq!(places.titles, [("en".into(), "Places".into())]);
        assert_eq!(places.counts, [("en".into(), "{n} places".into())]);
        assert_eq!(places.kind_type.as_deref(), Some("place"));
        assert_eq!(places.explorer, Some(false));
        assert_eq!(places.line, Some(true));
        assert_eq!(places.cards, Some(true));
        assert_eq!(config.terms_nav(), [("places".into(), Some(-2))]);
    }
}

#[test]
fn labels_accept_language_tables_and_false_hides_count() {
    for (value, expected) in [
        ("'People'", vec![("en".into(), "People".into())]),
        ("{ de = 'Leute', en = 'People' }", vec![("de".into(), "Leute".into()), ("en".into(), "People".into())]),
    ] {
        let (config, warnings) = parse_config(&format!("terms.people = {{ fields = ['editor'], title = {value}, count = {value} }}"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let kinds = config.terms_kinds();
        assert_eq!(kinds[2].titles, expected);
        assert_eq!(kinds[2].counts, expected);
    }
    let (config, warnings) = parse_config("terms.people = { fields = ['editor'], count = false }");
    assert!(warnings.is_empty());
    assert_eq!(config.terms_kinds()[2].counts, [("en".into(), String::new())]);
}

#[test]
fn bad_kind_values_warn_without_discarding_valid_neighbours() {
    let (config, warnings) = parse_config("site.lang = 'de'\nterms.broken = { fields = 'editor' }\nterms.missing = { title = 'Missing' }\nterms.people = { fields = ['editor'], count = 42, title = { en = 7 }, type = 7, explorer = 'yes', line = [], cards = 'yes' }\nterms.venues = { fields = ['location'], count = '{n} venues' }");
    assert_eq!(config.site_lang(), Some("de"));
    let kinds = config.terms_kinds();
    assert_eq!(kinds.iter().map(|k| k.key.as_str()).collect::<Vec<_>>(), ["authors", "tags", "people", "venues"]);
    assert_eq!(kinds[2].fields, ["editor"]);
    assert!(kinds[2].counts.is_empty());
    assert!(kinds[2].titles.is_empty());
    assert_eq!(kinds[2].kind_type, None);
    assert_eq!(kinds[2].explorer, None);
    assert_eq!(kinds[2].line, None);
    assert_eq!(kinds[2].cards, None);
    assert_eq!(kinds[3].counts, [("en".into(), "{n} venues".into())]);
    for key in ["[terms.broken] has no `fields`", "[terms.missing] has no `fields`", "[terms.people] count", "[terms.people] title", "[terms.people] type", "[terms.people] explorer", "[terms.people] line", "[terms.people] cards"] {
        assert!(warnings.iter().any(|w| w.contains(key)), "missing {key}: {warnings:?}");
    }
    assert_eq!(warnings.len(), 8);
}

#[test]
fn mixed_fields_keep_strings_and_non_bool_cards_warn() {
    for cards in ["'yes'", "7", "1.5", "['yes', 2]", "{ on = true }"] {
        let (config, warnings) = parse_config(&format!("terms.people = {{ fields = ['editor', 7, 'jury'], cards = {cards} }}"));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("[terms.people] fields"));
        assert_eq!(warnings[1], "[terms.people] cards must be true or false; ignoring it");
        let kinds = config.terms_kinds();
        assert_eq!(kinds[2].fields, ["editor", "jury"]);
        assert_eq!(kinds[2].cards, None);
    }
}

#[test]
fn invalid_site_or_terms_values_do_not_erase_other_settings() {
    let (config, warnings) = parse_config("site.lang = 7\nterms.people.fields = ['editor']");
    assert_eq!(config.site_lang(), None);
    assert_eq!(config.terms_kinds()[2].fields, ["editor"]);
    assert_eq!(warnings, ["[site] lang must be text; ignoring it"]);
    let (config, warnings) = parse_config("site.lang = 'de'\nterms = false");
    assert_eq!(config.site_lang(), Some("de"));
    assert_eq!(config.terms_kinds().len(), 2);
    assert_eq!(warnings, ["[terms] must be a table; ignoring it"]);
}

#[test]
fn unsupported_toml_1_config_reports_the_parser_problem() {
    // TOML 1.0 permits mixed arrays, including local times. basic-toml can
    // read mixed arrays, but cannot parse the time even in an unknown key.
    let (config, warnings) = parse_config("site.lang = 'de'\nhost_values = ['text', 07:32:00]");
    assert_eq!(config.site_lang(), None);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("config.toml with basic-toml"), "{warnings:?}");
    assert!(warnings[0].contains("invalid number at line 2"), "{warnings:?}");
}

#[test]
fn non_table_term_entries_do_not_become_positional_structs() {
    let (config, warnings) = parse_config("terms.empty = []\nterms.positional = [['editor']]\nterms.people.fields = ['jury']");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.terms_kinds().iter().map(|k| k.key.as_str()).collect::<Vec<_>>(), ["authors", "tags", "people"]);
}

#[test]
fn site_arrays_cannot_set_a_positional_language() {
    let (config, warnings) = parse_config("site = ['de']\nterms.people.fields = ['editor']");
    assert_eq!(config.site_lang(), None);
    assert_eq!(warnings, ["[site] must be a table; ignoring it"]);
    assert_eq!(config.terms_kinds()[2].fields, ["editor"]);
}

#[test]
fn a_nav_entry_needs_no_fields_so_a_built_in_can_join_the_nav() {
    let (config, warnings) = parse_config("[terms.authors]\nnav = true\nweight = 3");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.terms_nav(), [("authors".into(), Some(3))]);
    assert_eq!(config.terms_kinds()[0].fields, ["author"]);
}
