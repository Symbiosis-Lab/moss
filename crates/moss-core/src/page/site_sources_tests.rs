use super::*;

fn sources() -> SiteSources {
    SiteSources {
        files: vec!["z.md".into(), "index.md".into(), "a.md".into(), "child/index.md".into()],
        texts: HashMap::from([
            ("index.md".into(), "---\nlang: FR\n---\nHome".into()),
            ("a.md".into(), "---\nlang: de\n---\nArticle".into()),
        ]),
        ..Default::default()
    }
}

#[test]
fn only_site_text_paths_are_needed_to_assemble_the_bundle() {
    for home in [None, Some("z.MD"), Some("a.md")] {
        let mut input = sources();
        input.files.extend(["z.MD", "picture.png", "index.markdown", "nested/home.md"].map(str::to_string));
        for path in &input.files {
            input.texts.entry(path.clone()).or_insert_with(|| "---\nhome: true\nlang: es\n---".into());
        }
        for path in ["z.md", "z.MD"] {
            input.texts.insert(path.into(), "---\nlang: de\n---".into());
        }
        if let Some(path) = home {
            input.texts.insert(path.into(), "---\nhome: true\nlang: ja\n---".into());
        }
        // Texts absent from the file snapshot cannot elect a home either.
        input.texts.insert("absent.md".into(), "---\nhome: true\nlang: it\n---".into());
        let paths = site_text_paths(&input.files);
        assert_eq!(paths, ["a.md", "index.md", "z.MD", "z.md"]);
        let mut selected = input.clone();
        selected.texts.retain(|path, _| paths.contains(&path.as_str()));
        let all = site_bundle(input);
        let minimal = site_bundle(selected);
        assert_eq!(all.0.lang, if home.is_some() { "ja" } else { "fr" });
        assert_eq!(serde_json::to_value(&minimal).unwrap(), serde_json::to_value(&all).unwrap());
    }
    assert!(site_text_paths(&["nested/index.md".into(), "photo.png".into()]).is_empty());
}

#[test]
fn home_election_and_its_language() {
    let mut input = sources();
    assert_eq!(home_file(&input.files).as_deref(), Some("index.md"));
    assert_eq!(site_bundle(input.clone()).0.lang, "fr");
    input.texts.insert("z.md".into(), "---\nhome: true\nlang: ' ZH-Hant '\n---\nHome".into());
    input.texts.insert("child/index.md".into(), "---\nhome: true\nlang: es\n---".into());
    let (bundle, warnings) = site_bundle(input);
    assert_eq!(bundle.lang, "zh-hant");
    assert_eq!(bundle.files, ["a.md", "child/index.md", "index.md", "z.md"]);
    assert!(warnings.is_empty());
    assert_eq!(home_file(&["image.png".into(), "child/index.md".into()]), None);
}

#[test]
fn language_ladder_normalizes_and_falls_through() {
    for (explicit, home, detected, expected) in [
        (Some(" FR "), Some("de"), Some("es"), "fr"),
        (Some("english"), Some(" DE "), Some("es"), "de"),
        (None, Some(" ZH-Hant "), Some("es"), "zh-hant"),
        (None, Some("invalid"), Some(" ES "), "es"),
        (None, None, None, "en"),
        (Some("invalid"), Some("invalid"), Some("invalid"), "en"),
    ] {
        let lang = resolve_site_default_lang(explicit, || home.map(str::to_string), || detected.map(str::to_string));
        assert_eq!(lang, expected);
    }
    let lang = resolve_site_default_lang(Some("de"), || panic!("home read"), || panic!("prose detected"));
    assert_eq!(lang, "de", "a later rung is never read once an earlier one holds");
    assert_eq!(declared_lang_in_text("---\nlang: ' FR '\n---").as_deref(), Some("FR"));
    assert_eq!(declared_lang_in_text("---\nlang: invalid\n---"), None);
}

fn plugin(folder: &str, manifest: &str) -> PluginSource {
    PluginSource { folder: folder.into(), manifest_json: manifest.into(), entry_present: true, admission: Admission::Admitted }
}

#[test]
fn manifests_contribute_fields_styles_and_name_fallback() {
    let (bundle, warnings) = site_bundle(SiteSources {
        plugins: vec![
            plugin("z", r#"{"name":"zeta","entry":"main.js","contributes":{"frontmatter":{"fields":{"score":{"type":"integer"}}},"listing_styles":{"board":{}}}}"#),
            plugin("a", r#"{"name":"alpha","entry":"main.js","contributes":{"frontmatter":{"fields":{"score":{"type":"string"}}},"listing_styles":{"board":{},"rail":{}}}}"#),
            plugin("fallback", "{}"),
        ],
        theme_style_css: Some("@import 'other.css';\nbody {}".into()),
        ..Default::default()
    });
    assert_eq!(bundle.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["alpha", "fallback", "zeta"]);
    assert_eq!(bundle.listing_styles, ["board", "rail"]);
    assert_eq!(bundle.theme_css, "@import 'other.css';\nbody {}");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("alpha") && w.contains("zeta")));
    assert_eq!(bundle.inputs().schema.frontmatter.fields["score"].source.as_deref(), Some("alpha"));
}

#[test]
fn admission_and_invalid_plugins_warn_and_skip() {
    let mut refused = plugin("refused", r#"{"entry":"missing.js"}"#);
    refused.admission = Admission::Refused { sentence: "host denied execution".into() };
    refused.entry_present = false;
    let mut missing = plugin("missing", r#"{"entry":"absent.js"}"#);
    missing.entry_present = false;
    let (bundle, warnings) = site_bundle(SiteSources {
        plugins: vec![refused, missing, plugin("broken", "{"),
            plugin("reserved", r#"{"entry":"main.js","contributes":{"listing_styles":{"grid":{}}}}"#)],
        ..Default::default()
    });
    assert!(bundle.plugins.is_empty());
    assert_eq!(warnings.len(), 4);
    assert!(warnings.iter().any(|w| w.contains("refused") && w.contains("host denied execution") && !w.contains("not found")));
    assert!(warnings.iter().any(|w| w.contains("missing") && w.contains("absent.js")));
    assert!(warnings.iter().any(|w| w.contains("broken") && w.contains("invalid manifest")));
    assert!(warnings.iter().any(|w| w.contains("reserved") && w.contains("moss's own")));
}

#[test]
fn malformed_config_warns_and_uses_defaults() {
    let (bundle, warnings) = site_bundle(SiteSources { config_toml: Some("[broken".into()), ..Default::default() });
    assert_eq!(bundle.lang, "en");
    assert_eq!(bundle.declared_name_lists, ["author", "tags"]);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("config.toml"));
}

#[test]
fn terms_keep_sorted_kind_order_legacy_flags_and_collision_warnings() {
    let (bundle, warnings) = site_bundle(SiteSources {
        config_toml: Some("[terms]\nauthor = false\n[terms.zebra]\nfields = ['jury', 'editor']\n[terms.people]\nfields = ['editor', 'tag', 'title']\n".into()),
        ..Default::default()
    });
    assert_eq!(bundle.declared_name_lists, ["tags", "editor", "jury"]);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn config_arrays_of_tables_do_not_discard_site_settings() {
    let (bundle, warnings) = site_bundle(SiteSources {
        config_toml: Some("site.lang = 'de'\nterms.people.fields = ['editor']\n[[plugins]]\nname = 'first'\noptions = { enabled = true }\n[[plugins]]\nname = 'second'\n".into()),
        ..Default::default()
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(bundle.lang, "de");
    assert_eq!(bundle.declared_name_lists, ["author", "tags", "editor"]);
}
