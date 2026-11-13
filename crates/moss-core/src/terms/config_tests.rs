use super::*;

fn parse(text: &str) -> Vec<RawKind> {
    crate::page::site_config::parse_config(text).0.terms_kinds()
}

fn filtered(kinds: &[RawKind]) -> Vec<RawKind> {
    filter_fields(kinds).0.into_iter().map(|(mut kind, fields)| {
        kind.fields = fields;
        kind
    }).collect()
}

#[test]
fn built_in_kind_off_by_config_derives_nothing() {
    let cfg = parse("[terms]\ntags = false\n");
    let kinds = filtered(&cfg);
    let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
    assert!(tags.fields.is_empty(), "tags = false empties the built-in kind: {:?}", tags.fields);
    let authors = kinds.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
    assert_eq!(authors.fields, vec!["author".to_string()], "authors kind untouched");
}

#[test]
fn a_declared_field_is_a_name_list_field_for_the_site() {
    let cfg =
        parse("[terms.people]\nfields = [\"editor\", \"ghost\"]\n");
    let kinds = filtered(&cfg);
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
        parse("[terms.people]\nfields = [\"editor\", \"title\"]\n");
    let kinds = filtered(&cfg);
    let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
    assert_eq!(people.fields, vec!["editor".to_string()], "title is not a name list");
}

#[test]
fn a_declared_name_whose_claim_key_is_a_built_ins_is_refused() {
    // `tag` would claim `tag_page:`, the key `tags` already owns.
    let cfg = parse("[terms.people]\nfields = [\"tag\"]\n");
    let kinds = filtered(&cfg);
    let people = kinds.iter().find(|k| k.key == "people").expect("people kind present");
    assert!(people.fields.is_empty(), "tag collides with tags: {:?}", people.fields);
}

#[test]
fn place_is_a_place_field_unless_another_kind_lists_location() {
    let alone = parse("[terms.places]\ntype = \"place\"\nfields = [\"place\"]\n");
    let kinds = filtered(&alone);
    let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
    assert_eq!(places.fields, vec!["place".to_string()]);

    let both = parse(
        "[terms.places]\nfields = [\"place\"]\n[terms.venues]\nfields = [\"location\"]\n",
    );
    let kinds = filtered(&both);
    let places = kinds.iter().find(|k| k.key == "places").expect("places kind present");
    assert!(places.fields.is_empty(), "one place_page: would claim in both kinds");
}

#[test]
fn a_built_in_table_without_fields_keeps_its_built_in_field() {
    let (config, warnings) = crate::page::site_config::parse_config("[terms.tags]\nline = true\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    let kinds = config.terms_kinds();
    let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
    assert_eq!(tags.fields, ["tags"]);
    assert_eq!(tags.line, Some(true));
}

#[test]
fn a_table_without_fields_does_not_turn_a_disabled_built_in_back_on() {
    let cfg = parse("[terms]\nauthor = false\n[terms.authors]\nnav = true\n");
    let authors = cfg.iter().find(|k| k.key == AUTHOR_NS).expect("authors kind present");
    assert!(authors.fields.is_empty(), "{:?}", authors.fields);
}

#[test]
fn a_built_in_table_whose_fields_is_not_a_list_warns_and_is_skipped() {
    let (config, warnings) = crate::page::site_config::parse_config("[terms.tags]\nfields = \"topic\"\nline = true\n");
    assert_eq!(warnings, ["[terms.tags] has no `fields` array; skipping"]);
    let kinds = config.terms_kinds();
    let tags = kinds.iter().find(|k| k.key == TAGS_NS).expect("tags kind present");
    assert_eq!((tags.fields.as_slice(), tags.line), (["tags".to_string()].as_slice(), None));
}

#[test]
fn a_declared_kind_without_fields_is_still_skipped() {
    let (config, warnings) = crate::page::site_config::parse_config("[terms.people]\nline = true\n");
    assert_eq!(warnings, ["[terms.people] has no `fields` array; skipping"]);
    assert!(config.terms_kinds().iter().all(|k| k.key != "people"));
}

#[test]
fn config_shapes_preserve_term_values() {
    for text in [
        "terms.people = { fields = ['editor'], title = { en = 'People', de = 'Leute' }, count = false, cards = true }",
        "terms.people.fields = ['editor']\nterms.people.title.en = 'People'\nterms.people.title.de = 'Leute'\nterms.people.count = false\nterms.people.cards = true",
        "[terms.people]\nfields = ['editor']\ncount = false\ncards = true\n[terms.people.title]\nen = 'People'\nde = 'Leute'",
    ] {
        let (config, warnings) = crate::page::site_config::parse_config(text);
        let kinds = config.terms_kinds();
        assert!(warnings.is_empty(), "{warnings:?}");
        let people = kinds.iter().find(|kind| kind.key == "people").unwrap();
        assert_eq!(people.fields, ["editor"]);
        assert_eq!(people.titles, [("de".into(), "Leute".into()), ("en".into(), "People".into())]);
        assert_eq!(people.counts, [("en".into(), String::new())]);
        assert_eq!(people.cards, Some(true));
    }
}
