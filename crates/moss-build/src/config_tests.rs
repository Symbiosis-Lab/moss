use super::*;

/// A config as an advanced user actually writes one: section comments, a
/// trailing comment on a key, keys in the order that made sense to them
/// (`[build]` before `[site]`, `lang` after `search`), single quotes, an
/// array broken over lines, and a blank line where they wanted a breath.
///
/// Every test below reads THIS text, so a reader that only works on
/// moss-generated formatting fails here.
const HAND_WRITTEN: &str = "\
# 河灣 — site configuration.
# Hand-written; the modals are for people in a hurry.

schema_version = 5
environment = 'staging'   # local when the VPS is down

[build]
passthrough = [
  \"static\",       # served verbatim
  \"!drafts\",
]
keep_generations = 3
prune_orphaned_images = false

[site]
search   = true
math     = false
lang     = 'zh-hans'
content_width = \"wide\"

# Comments are opt-in, per service.
[services.comments]
server_url = \"https://comments.example.org\"

[channels.email.send_mode]
mode = \"scope\"
value = \"zh-hans\"
";

#[test]
fn reads_every_shape_out_of_a_hand_written_config() {
    let config = ConfigFile::parse(HAND_WRITTEN).unwrap();

    assert_eq!(config.site_str("lang"), Some("zh-hans"));
    assert_eq!(config.site_str("content_width"), Some("wide"));
    assert_eq!(config.site_bool("search"), Some(true));
    assert_eq!(config.site_bool("math"), Some(false));
    assert_eq!(config.environment(), Some("staging"));
    assert_eq!(config.build_passthrough(), vec!["static", "!drafts"]);
    assert_eq!(config.build_keep_generations(), Some(3));
    assert_eq!(config.build_prune_orphaned_images(), Some(false));

    let send_mode = config
        .section(&["channels", "email", "send_mode"])
        .expect("send_mode section");
    assert_eq!(send_mode.get("mode").and_then(|v| v.as_str()), Some("scope"));
}

#[test]
fn an_absent_key_reads_as_absent_not_as_false() {
    // The distinction the whole reader turns on: most site knobs default ON,
    // so a reader that folded "absent" into `false` would silently switch off
    // implicit figures, link previews and heading anchors for every site that
    // never touched them.
    let config = ConfigFile::parse("[site]\nsearch = true\n").unwrap();
    assert_eq!(config.site_bool("implicit_figure"), None);
    assert_eq!(config.site_bool("search"), Some(true));
}

#[test]
fn an_empty_config_answers_none_to_everything() {
    let config = ConfigFile::empty();
    assert_eq!(config.site_str("lang"), None);
    assert_eq!(config.site_bool("comments"), None);
    assert_eq!(config.environment(), None);
    assert!(config.build_passthrough().is_empty());
    assert_eq!(config.build_keep_generations(), None);
    assert_eq!(config.section(&["services"]), None);
}

#[test]
fn a_malformed_config_is_an_error_not_an_empty_one() {
    // Reading a broken file as "no settings" is how a caller ends up writing
    // its defaults over the file the user broke by hand and can still fix.
    let err = ConfigFile::parse("[site\nlang = 'zh'").unwrap_err();
    assert!(err.contains("Failed to parse config.toml"), "got {err}");
}

#[test]
fn a_malformed_build_knob_reads_as_absent_rather_than_failing() {
    // `keep_generations` and `prune_orphaned_images` are knobs, not gates:
    // whatever the user typed, the build has a default and must still run.
    let config =
        ConfigFile::parse("[build]\nkeep_generations = \"three\"\nprune_orphaned_images = 1\n")
            .unwrap();
    assert_eq!(config.build_keep_generations(), None);
    assert_eq!(config.build_prune_orphaned_images(), None);
}

#[test]
fn a_negative_generation_count_clamps_to_zero() {
    let config = ConfigFile::parse("[build]\nkeep_generations = -4\n").unwrap();
    assert_eq!(config.build_keep_generations(), Some(0));
}

#[test]
fn passthrough_ignores_non_string_entries_instead_of_failing() {
    let config = ConfigFile::parse("[build]\npassthrough = [\"static\", 7, \"!drafts\"]\n").unwrap();
    assert_eq!(config.build_passthrough(), vec!["static", "!drafts"]);
}

#[test]
fn section_walks_a_missing_path_without_panicking() {
    let config = ConfigFile::parse(HAND_WRITTEN).unwrap();
    assert!(config.section(&["services", "analytics"]).is_none());
    assert!(config.section(&["site", "lang", "nope"]).is_none());
}

// The hand-editability ruling is NOT asserted here, deliberately.
// A read-side "the text is unchanged afterwards" test in this crate cannot
// fail: `parse` takes `&str` and every accessor takes `&self`, so the borrow
// checker already forbids the mutation, and no edit to production code can
// turn such a test red. It was written, reviewed, and deleted for exactly
// that reason. The ruling is asserted where a writer actually exists — the
// app's own config hand-editability test, which reads a
// hand-written config through this reader and then compares the file
// byte-for-byte after a settings modal writes to it.

/// Open-CLI slice 3: the one parse funnel migrates in memory, so a
/// host that never persists (moss-cli) still reads a legacy config at its
/// migrated meaning. Mirrors the parity fixture's v0 shape — pre-v1
/// `[features]`/`[analytics]` and no `schema_version` — whose analytics slot
/// and RSS footer are exactly what diverge if one host migrates and the
/// other does not (a silent-defaults hazard).
#[test]
fn parse_migrates_a_v0_config_in_memory() {
    let v0 = "[site]\nlang = \"en\"\n\n[features]\nrss = true\n\n[analytics]\nscript = \"<script src=\\\"https://stats.example.org/js/script.js\\\"></script>\"\n";
    let config = ConfigFile::parse(v0).unwrap();
    assert_eq!(
        config.section(&["services", "analytics", "script"]).and_then(|v| v.as_str()),
        Some("<script src=\"https://stats.example.org/js/script.js\"></script>"),
        "pre-v1 [analytics] must read as [services.analytics]"
    );
    assert_eq!(
        config.section(&["site", "rss_footer"]).and_then(|v| v.as_bool()),
        Some(true),
        "features.rss must read as [site].rss_footer"
    );
    assert!(config.section(&["features"]).is_none(), "[features] is dropped by the transform");
}

/// `parse`'s `VersionAhead` fallback logs and keeps the RAW table — which
/// means the original `schema_version` survives into `self.root` untouched,
/// so `schema_version_ahead()` recovers the real answer from the SAME parse
/// this test constructs `config` from, no second read. Ablated by reverting
/// `schema_version_ahead` to always return `None`: goes red on the `Some`
/// assertion.
#[test]
fn schema_version_ahead_reads_off_the_same_parse_that_swallowed_the_error() {
    let future = crate::config::migrations::CURRENT_VERSION + 2;
    let config = ConfigFile::parse(&format!("schema_version = {future}\n")).unwrap();
    assert_eq!(config.schema_version_ahead(), Some(future));

    let current = ConfigFile::parse(&format!(
        "schema_version = {}\n",
        crate::config::migrations::CURRENT_VERSION
    ))
    .unwrap();
    assert_eq!(current.schema_version_ahead(), None);
}

#[test]
fn redirects_table_is_optional() {
    let cfg = ConfigFile::parse("schema_version = 6\n[site]\nlang = \"en\"\n").unwrap();
    assert!(cfg.declared_redirects().is_empty());
}

#[test]
fn redirects_are_read_as_old_to_new_pairs_with_any_slash_spelling() {
    let cfg = ConfigFile::parse(
        "schema_version = 6\n[redirects]\n\"/old/\" = \"/new/\"\n\"gone.html\" = \"https://example.com/x\"\n",
    )
    .unwrap();
    assert_eq!(
        cfg.declared_redirects(),
        vec![
            ("/old/".to_string(), "/new/".to_string()),
            ("gone.html".to_string(), "https://example.com/x".to_string()),
        ]
    );
}

#[test]
fn a_redirect_that_is_not_a_string_is_one_advisory_and_the_rest_still_read() {
    let _ = crate::build::cli_output::take_cli_problems();
    let cfg =
        ConfigFile::parse("schema_version = 6\n[redirects]\n\"/a/\" = 3\n\"/b/\" = \"/c/\"\n").unwrap();
    assert_eq!(cfg.declared_redirects(), vec![("/b/".to_string(), "/c/".to_string())]);
    assert_eq!(crate::build::cli_output::take_cli_problems(), 1);
}

#[test]
fn a_redirects_value_that_is_not_a_table_is_an_advisory_not_a_crash() {
    let _ = crate::build::cli_output::take_cli_problems();
    let cfg = ConfigFile::parse("schema_version = 6\nredirects = \"nope\"\n").unwrap();
    assert!(cfg.declared_redirects().is_empty());
    assert_eq!(crate::build::cli_output::take_cli_problems(), 1);
}

#[test]
fn field_decls_reads_a_choice_with_a_label_table() {
    let config = ConfigFile::parse(
        "[fields.kind]\ntype = \"choice\"\nof = { Building = { en = \"Built\", de = \"Errichtet\" }, Service = \"Founded\" }\nplace = \"eyebrow\"\nin = [\"Housing\"]\n",
    )
    .unwrap();
    let (decls, problems) = config.field_decls();
    assert!(problems.is_empty(), "got: {problems:?}");
    assert_eq!(decls.len(), 1);
    let kind = &decls[0];
    assert_eq!(kind.name, "kind");
    assert_eq!(kind.kind, moss_core::fields::FieldKind::Choice);
    assert_eq!(
        kind.choices,
        vec![
            moss_core::fields::Choice {
                value: "Building".into(),
                labels: vec![("de".into(), "Errichtet".into()), ("en".into(), "Built".into())],
            },
            moss_core::fields::Choice { value: "Service".into(), labels: vec![("en".into(), "Founded".into())] },
        ]
    );
    assert_eq!(kind.place, Some(moss_core::fields::Place::Eyebrow));
    assert_eq!(kind.scope, vec!["Housing".to_string()]);
}

#[test]
fn field_decls_plumbing_builtin_cannot_take_a_place() {
    let config = ConfigFile::parse("[fields.url]\nplace = \"eyebrow\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(decls.is_empty());
    assert!(problems.iter().any(|p| p.contains("[fields.url]") && p.contains("description and publisher")), "got: {problems:?}");
}

#[test]
fn field_decls_choice_table_keeps_the_order_the_file_writes() {
    let config = ConfigFile::parse(
        "[fields.kind]\ntype = \"choice\"\nof = { Service = \"Founded\", Building = \"Built\", Event = \"Held\" }\n",
    )
    .unwrap();
    let (decls, _) = config.field_decls();
    let values: Vec<&str> = decls[0].choices.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(values, ["Service", "Building", "Event"]);
}

#[test]
fn field_decls_keep_the_order_the_file_writes_and_read_a_label() {
    let config = ConfigFile::parse(
        "[fields.years]\ntype = \"period\"\n[fields.architect]\ntype = \"text\"\nlabel = { en = \"Architect\", de = \"Architektin\" }\n[fields.district]\ntype = \"text\"\nlabel = \"Quarter\"\n",
    )
    .unwrap();
    let (decls, problems) = config.field_decls();
    assert!(problems.is_empty(), "got: {problems:?}");
    let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["years", "architect", "district"]);
    assert_eq!(decls[1].label_for("de"), "Architektin");
    assert_eq!(decls[2].label_for("de"), "Quarter");
    assert_eq!(decls[0].label_for("de"), "Years");
}

#[test]
fn field_decls_choice_from_a_list_labels_each_value_with_itself() {
    let config = ConfigFile::parse("[fields.kind]\ntype = \"choice\"\nof = [\"Building\", \"Service\"]\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(problems.is_empty(), "got: {problems:?}");
    assert_eq!(
        decls[0].choices,
        vec![
            moss_core::fields::Choice { value: "Building".into(), labels: vec![("en".into(), "Building".into())] },
            moss_core::fields::Choice { value: "Service".into(), labels: vec![("en".into(), "Service".into())] },
        ]
    );
}

#[test]
fn field_decls_unknown_type_is_a_problem_and_dropped() {
    let config = ConfigFile::parse("[fields.size]\ntype = \"integer\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(decls.is_empty());
    assert_eq!(problems.len(), 1, "got: {problems:?}");
    assert!(problems[0].contains("[fields.size] has an unknown type \"integer\"; one of: text, number"), "{}", problems[0]);
}

#[test]
fn field_decls_description_takes_a_place() {
    let config = ConfigFile::parse("[fields.description]\nplace = \"eyebrow\"\nlabel = \"Summary\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(problems.is_empty(), "got: {problems:?}");
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].kind, moss_core::fields::FieldKind::Text);
    assert_eq!(decls[0].place, Some(moss_core::fields::Place::Eyebrow));
    assert_eq!(decls[0].label_for("en"), "Summary");
}

#[test]
fn field_decls_a_builtin_takes_only_place_and_label() {
    let config = ConfigFile::parse("[fields.description]\ntype = \"text\"\nin = [\"Housing\"]\nplace = \"subtitle\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert_eq!(decls.len(), 1, "the declaration is kept: {problems:?}");
    assert_eq!(decls[0].place, Some(moss_core::fields::Place::Subtitle));
    assert!(decls[0].scope.is_empty());
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("has \"type\"; built-in fields take only place and label")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("has \"in\"; built-in fields take only place and label")), "{problems:?}");
}

#[test]
fn field_decls_unknown_key_and_bad_place_keep_the_declaration() {
    let config = ConfigFile::parse("[fields.years]\ntype = \"period\"\nsize = 3\n[fields.mood]\ntype = \"text\"\nplace = \"sidebar\"\n")
        .unwrap();
    let (decls, problems) = config.field_decls();
    assert_eq!(decls.len(), 2, "both declarations are kept");
    assert!(problems.iter().any(|p| p.contains("[fields.years] has an unknown key \"size\"")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("[fields.mood] has an unknown place")), "{problems:?}");
    let mood = decls.iter().find(|d| d.name == "mood").unwrap();
    assert_eq!(mood.place, None);
}

#[test]
fn field_decls_choice_without_of_is_dropped() {
    let config = ConfigFile::parse("[fields.kind]\ntype = \"choice\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(decls.is_empty());
    assert_eq!(problems.len(), 1, "{problems:?}");
}

#[test]
fn field_decls_fields_that_is_not_a_table_of_tables_is_one_problem() {
    let config = ConfigFile::parse("fields = \"kind\"\n").unwrap();
    let (decls, problems) = config.field_decls();
    assert!(decls.is_empty());
    assert_eq!(problems.len(), 1);
}

#[test]
fn field_decls_url_text_is_per_language_and_only_a_url_takes_it() {
    let config = ConfigFile::parse(
        "[fields.item_url]\ntype = \"url\"\ntext = { en = \"View the item\", de = \"Zum Eintrag\" }\n[fields.year]\ntype = \"number\"\ntext = \"Year\"\n",
    )
    .unwrap();
    let (decls, problems) = config.field_decls();
    assert_eq!(decls[0].text_for("de"), Some("Zum Eintrag"));
    assert_eq!(decls[0].text_for("fr"), Some("View the item"));
    assert!(decls[1].text.is_empty());
    assert!(problems.iter().any(|p| p.contains("[fields.year] has `text`")), "{problems:?}");
}
