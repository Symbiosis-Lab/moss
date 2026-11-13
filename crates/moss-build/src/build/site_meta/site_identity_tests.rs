use super::*;

fn registry() -> moss_core::contract::tokens::Tokens {
    moss_core::contract::tokens::load_tokens().unwrap()
}

fn colour(tokens: &Value, name: &str) -> Option<(String, String)> {
    let v = &tokens["color"][name]["$value"];
    Some((v["light"].as_str()?.to_string(), v["dark"].as_str()?.to_string()))
}

fn pair(l: &str, d: &str) -> Option<(String, String)> {
    Some((l.to_string(), d.to_string()))
}

#[test]
fn an_unthemed_site_publishes_moss_s_own_values() {
    let t = tokens(&registry(), None, "en", ColorScheme::Auto);
    assert_eq!(colour(&t, "moss-color-bg"), pair("#faf8f5", "#1c1914"));
    assert_eq!(t["$extensions"]["moss"]["version"], TOKENS_VERSION);
    assert!(t["typography"]["moss-font-heading"]["$value"].as_str().unwrap().ends_with("serif"));
    // An alias resolves through moss's own chain: the chrome accent is the content accent.
    assert_eq!(colour(&t, "moss-color-ui-accent"), colour(&t, "moss-color-accent"));
}

#[test]
fn the_theme_wins_in_both_schemes_and_a_dark_rule_wins_in_dark() {
    let css = ":root { --moss-color-bg: #ffffff; --moss-color-accent: #b94a28; }\n:root[data-theme=\"dark\"] { --moss-color-accent: #e07a5a; }";
    let t = tokens(&registry(), Some(css), "en", ColorScheme::Auto);
    assert_eq!(colour(&t, "moss-color-bg"), pair("#ffffff", "#ffffff"), "a :root value holds in dark too, as on the page");
    assert_eq!(colour(&t, "moss-color-accent"), pair("#b94a28", "#e07a5a"));
    assert_eq!(colour(&t, "moss-color-ui-accent"), pair("#b94a28", "#e07a5a"), "the alias follows the theme's accent");
}

#[test]
fn a_theme_alias_is_followed() {
    let css = ":root { --brand: #123456; --moss-color-accent: var(--brand); --moss-font-heading: var(--display); --display: Georgia, serif; }";
    let t = tokens(&registry(), Some(css), "en", ColorScheme::Auto);
    assert_eq!(colour(&t, "moss-color-accent"), pair("#123456", "#123456"));
    assert_eq!(t["typography"]["moss-font-heading"]["$value"], "Georgia, serif");
}

#[test]
fn a_value_set_only_outside_the_root_leaves_the_root_value() {
    let css = "@media (prefers-color-scheme: dark) { :root { --moss-color-bg: #000000; } }\nbody[data-section] { --moss-color-bg: #ff0000; }";
    let t = tokens(&registry(), Some(css), "en", ColorScheme::Auto);
    assert_eq!(colour(&t, "moss-color-bg"), pair("#faf8f5", "#1c1914"));
}

#[test]
fn what_cannot_be_reduced_to_a_plain_value_is_left_out() {
    let css = ":root { --moss-color-accent: color-mix(in oklch, red, blue); --moss-color-surface: var(--nowhere); }";
    let t = tokens(&registry(), Some(css), "en", ColorScheme::Auto);
    assert_eq!(colour(&t, "moss-color-accent"), None, "color-mix is not guessed");
    assert_eq!(colour(&t, "moss-color-surface"), None, "a broken var() chain is not guessed");
    assert_eq!(colour(&t, "moss-color-accent-quiet"), None, "a translucent colour is not written as opaque");
    assert!(colour(&t, "moss-color-text").is_some());
}

#[test]
fn only_the_site_s_own_icon_is_its_icon() {
    let tc = ThemeColor::of_theme(None);
    let base = Identity { name: "Harbor Notes", lang_tag: "en", theme_color: &tc, color_scheme: ColorScheme::Auto, own_icon: None };
    let m = manifest(&base);
    assert_eq!(m["name"], "Harbor Notes");
    assert_eq!(m["lang"], "en");
    assert_eq!(m["start_url"], "./");
    assert_eq!(m["background_color"], tc.light);
    assert!(m.get("icons").is_none(), "moss's default mark is not the site's icon");

    let own = manifest(&Identity { own_icon: Some(OwnIcon { file: "favicon.svg", raster_sizes: &[16, 32, 180] }), ..base });
    assert_eq!(
        own["icons"],
        json!([
            { "src": "assets/favicon-180.png", "sizes": "180x180", "type": "image/png" },
            { "src": "assets/favicon.svg", "sizes": "any", "type": "image/svg+xml" },
        ])
    );
}

/// Build `files` (path, body) as a site and return its output directory and manifest.
fn build(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf, crate::build::manifest::SealedManifest) {
    use crate::build::render::blocking::generate_blocking_content;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for (rel, body) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let out = root.join(".moss").join("build.nosync").join("site");
    std::fs::create_dir_all(&out).unwrap();
    let structure = crate::build::scan::scan::scan_folder(root.to_str().unwrap()).unwrap();
    let mut pending = crate::build::manifest::PendingManifest::for_build(Default::default(), crate::build::cloud_ledger::InputEvidence::new(&root));
    generate_blocking_content(
        &crate::vault::paths::VaultRoot::resolve(&root),
        &structure,
        &out,
        None,
        None,
        true,
        crate::build::render::SiteConfig::default(),
        &mut pending,
    )
    .unwrap();
    (tmp, out, pending.seal())
}

fn json_at(out: &std::path::Path, rel: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(out.join(rel)).unwrap()).unwrap()
}

#[test]
fn every_build_writes_both_files_and_every_page_links_the_manifest() {
    let (_tmp, out, sealed) = build(&[
        ("index.md", "---\ntitle: Harbor Notes\n---\n\nHello.\n"),
        (".moss/theme/style.css", ":root { --moss-color-accent: #b94a28; }\n"),
    ]);
    for rel in ["manifest.webmanifest", "_moss/tokens.json"] {
        let written = std::fs::read(out.join(rel)).unwrap();
        assert_eq!(sealed.held_bytes(rel), Some(written.as_slice()), "{rel}: a later build's stage copy must not ship under this build's hash");
    }
    assert_eq!(json_at(&out, "manifest.webmanifest")["name"], "Harbor Notes");
    let t = json_at(&out, "_moss/tokens.json");
    assert_eq!(t["color"]["moss-color-accent"]["$value"]["light"], "#b94a28", "the theme's stylesheet reaches the tokens");
    let page = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(page.contains(r#"<link rel="manifest" href="/manifest.webmanifest">"#), "{page}");
}

#[test]
fn every_icon_the_manifest_names_is_a_file_the_build_writes() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#1d4f91"/></svg>"##;
    let (_tmp, out, _) = build(&[("index.md", "---\ntitle: Harbor Notes\n---\n\nHello.\n"), ("assets/favicon.svg", svg)]);
    let icons = json_at(&out, "manifest.webmanifest")["icons"].as_array().cloned().unwrap_or_default();
    assert!(icons.iter().any(|i| i["type"] == "image/png"), "{icons:?}");
    for icon in &icons {
        let src = icon["src"].as_str().unwrap();
        assert!(out.join(src).is_file(), "{src} is named but not written");
    }
}

#[test]
fn a_han_site_publishes_the_han_font_stacks_its_pages_use() {
    let t = tokens(&registry(), None, "zh-Hans", ColorScheme::Auto);
    let heading = t["typography"]["moss-font-heading"]["$value"].as_str().unwrap();
    assert!(heading.starts_with("'Source Han Serif SC'"), "{heading}");
    let latin = tokens(&registry(), None, "en", ColorScheme::Auto);
    assert_ne!(latin["typography"]["moss-font-heading"], t["typography"]["moss-font-heading"]);
}

#[test]
fn a_site_fixed_to_one_scheme_publishes_that_side_only() {
    let css = ":root { --moss-color-bg: #ffffff; }\n:root[data-theme=\"dark\"] { --moss-color-bg: #101010; }";
    let t = tokens(&registry(), Some(css), "en", ColorScheme::Dark);
    assert_eq!(colour(&t, "moss-color-bg"), pair("#101010", "#101010"));
    let tc = ThemeColor::of_theme(Some(css));
    let m = manifest(&Identity { name: "Night", lang_tag: "en", theme_color: &tc, color_scheme: ColorScheme::Dark, own_icon: None });
    assert_eq!(m["background_color"], "#101010");
}

#[test]
fn an_opaque_colour_is_read_in_any_notation_and_a_translucent_one_is_not() {
    assert_eq!(opaque_hex("rgba(29, 79, 145, 1)").as_deref(), Some("#1d4f91"));
    assert_eq!(opaque_hex("rgb(29 79 145 / 1)").as_deref(), Some("#1d4f91"));
    assert_eq!(opaque_hex("hsl(120, 100%, 25%)").as_deref(), Some("#008000"));
    assert_eq!(opaque_hex("rebeccapurple").as_deref(), Some("#663399"));
    for translucent in ["rgba(0, 0, 0, 0.5)", "rgb(0 0 0 / 50%)", "#0008", "transparent", "color-mix(in oklch, red, blue)", "#fff junk"] {
        assert_eq!(opaque_hex(translucent), None, "{translucent}");
    }
}

#[test]
fn a_theme_font_by_language_is_published_for_that_language_only() {
    let css = ":root { --moss-font-body: 'Field Sans', sans-serif; }\n\
               html:is([lang|=\"zh\"], [lang|=\"ja\"], [lang|=\"ko\"]) { --moss-font-body: 'Field Sans', var(--moss-font-body-han); }\n\
               html:lang(ja) { --moss-font-heading: Georgia, serif; }";
    let body = |lang: &str| tokens(&registry(), Some(css), lang, ColorScheme::Auto)["typography"]["moss-font-body"]["$value"].as_str().map(str::to_string);
    let heading = |lang: &str| tokens(&registry(), Some(css), lang, ColorScheme::Auto)["typography"]["moss-font-heading"]["$value"].as_str().map(str::to_string);
    assert_eq!(body("en").as_deref(), Some("'Field Sans', sans-serif"));
    assert_eq!(body("zh-Hant"), None, "the page draws the Han stack behind the theme's face, which the build cannot reduce to one value; not the :root stack");
    assert_eq!(heading("ja").as_deref(), Some("Georgia, serif"));
    assert_ne!(heading("en").as_deref(), Some("Georgia, serif"));
}
