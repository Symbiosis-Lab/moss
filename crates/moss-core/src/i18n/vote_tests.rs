use super::*;

#[test]
fn prose_that_did_not_classify_counts_toward_the_floor_only() {
    assert_eq!(language_vote([(None, 30)]), None);
    assert_eq!(language_vote([(Some(Language::En), 10)]), None);
    assert_eq!(language_vote([(Some(Language::En), 10), (None, 20)]), Some(Language::En));
}

#[test]
fn the_heavier_language_wins_and_a_tie_breaks_on_the_code_in_any_order() {
    assert_eq!(language_vote([(Some(Language::En), 30), (Some(Language::ZhHant), 40)]), Some(Language::ZhHant));
    let forward = language_vote([(Some(Language::En), 30), (Some(Language::ZhHans), 30)]);
    let reversed = language_vote([(Some(Language::ZhHans), 30), (Some(Language::En), 30)]);
    assert_eq!(forward, reversed);
    assert!(forward.is_some());
}

fn langs(paths: &[&str], declared: &[(&str, &str)], site_lang_declared: bool, detected: &[(&str, Language)]) -> HashMap<String, Language> {
    let declared: HashMap<String, String> = declared.iter().map(|(p, c)| (p.to_string(), c.to_string())).collect();
    let detected: HashMap<&str, Language> = detected.iter().copied().collect();
    folder_languages(
        paths.iter().copied(),
        &|p| declared.get(p).cloned(),
        &|p| p.ends_with("footer.md"),
        site_lang_declared,
        &mut |_, files| language_vote(files.iter().map(|f| (detected.get(f).copied(), 40))),
    )
}

#[test]
fn a_folder_index_declaration_outranks_the_vote_and_a_declared_site_skips_the_vote() {
    let paths = ["notes/index.md", "notes/one.md", "essays/one.md"];
    let detected = [("notes/one.md", Language::ZhHant), ("essays/one.md", Language::ZhHant)];

    let got = langs(&paths, &[("notes/index.md", "en")], false, &detected);
    assert_eq!(got.get("notes"), Some(&Language::En));
    assert_eq!(got.get("essays"), Some(&Language::ZhHant));

    let declared_site = langs(&paths, &[("notes/index.md", "en")], true, &detected);
    assert_eq!(declared_site.get("notes"), Some(&Language::En));
    assert_eq!(declared_site.get("essays"), None);
}

#[test]
fn a_host_with_no_detector_gets_only_declarations() {
    let got = langs(&["notes/index.md", "essays/one.md"], &[("notes/index.md", "zh-hant")], false, &[]);
    assert_eq!(got.get("notes"), Some(&Language::ZhHant));
    assert_eq!(got.get("essays"), None);
}

#[test]
fn named_trees_suffixed_and_slot_only_files_take_no_part() {
    let detected = [("en/a.md", Language::ZhHant), ("posts/a.zh-hant.md", Language::ZhHant), ("posts/footer.md", Language::ZhHant)];
    let got = langs(&["en/a.md", "posts/a.zh-hant.md", "posts/footer.md"], &[], false, &detected);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn only_the_index_file_itself_declares_for_its_folder() {
    let got = langs(&["notes/myindex.md", "notes/index.md"], &[("notes/myindex.md", "zh-hant")], true, &[]);
    assert_eq!(got.get("notes"), None, "{got:?}");
}
