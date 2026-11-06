use super::*;

const SITE: &str = "https://example.com";

fn preset(policy: Option<&str>) -> AiSettings {
    AiSettings::resolve(policy, None, None, None)
}

fn switches(search: bool, answers: bool, training: bool) -> AiSettings {
    AiSettings::resolve(None, Some(search), Some(answers), Some(training))
}

// ---- A site with no switch keeps the files ai_policy has always produced ----

const EXPECTED_STANDARD: &str = "\
User-agent: *
Content-Signal: search=yes, ai-input=yes, ai-train=no
Allow: /
User-agent: GPTBot
Disallow: /
User-agent: Google-Extended
Disallow: /
User-agent: ClaudeBot
Disallow: /
User-agent: CCBot
Disallow: /
User-agent: Bytespider
Disallow: /
User-agent: meta-externalagent
Disallow: /
User-agent: Amazonbot
Disallow: /
User-agent: Applebot-Extended
Disallow: /

Sitemap: https://example.com/sitemap.xml
";

#[test]
fn standard_preset_robots_txt_is_unchanged_byte_for_byte() {
    assert_eq!(preset(Some("standard")).robots_txt(SITE), EXPECTED_STANDARD);
    assert_eq!(preset(None).robots_txt(SITE), EXPECTED_STANDARD);
}

#[test]
fn unrecognized_policy_falls_back_to_standard() {
    assert_eq!(preset(Some("bogus")).robots_txt(SITE), EXPECTED_STANDARD);
}

#[test]
fn unrestricted_preset_robots_txt_is_unchanged_byte_for_byte() {
    let expected = "\
User-agent: *
Content-Signal: search=yes, ai-input=yes, ai-train=yes
Allow: /

Sitemap: https://example.com/sitemap.xml
";
    assert_eq!(preset(Some("unrestricted")).robots_txt(SITE), expected);
}

#[test]
fn restricted_preset_robots_txt_is_unchanged_byte_for_byte() {
    let expected = "\
User-agent: *
Content-Signal: search=no, ai-input=no, ai-train=no
Disallow: /

Sitemap: https://example.com/sitemap.xml
";
    assert_eq!(preset(Some("restricted")).robots_txt(SITE), expected);
}

#[test]
fn sitemap_url_has_no_double_slash() {
    for s in [preset(None), switches(true, true, false)] {
        let robots = s.robots_txt("https://example.com/");
        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"), "{robots}");
        assert!(!robots.contains("example.com//"), "{robots}");
    }
}

#[test]
fn a_preset_adds_no_meta_tag_reservation_file_or_llms_refusal() {
    for policy in ["standard", "unrestricted", "restricted"] {
        let s = preset(Some(policy));
        assert_eq!(s.robots_tag(false), None, "{policy}");
        assert_eq!(s.tdmrep_json(), None, "{policy}");
    }
    // A draft keeps the noindex it has always had.
    assert_eq!(
        preset(None).robots_tag(true).as_deref(),
        Some("<meta name=\"robots\" content=\"noindex\">")
    );
}

// ---- Switches ----

#[test]
fn presets_name_a_switch_state() {
    assert_eq!(Preset::Standard.uses(), Uses { search: true, answers: true, training: false });
    assert_eq!(Preset::Unrestricted.uses(), Uses { search: true, answers: true, training: true });
    assert_eq!(Preset::Restricted.uses(), Uses { search: false, answers: false, training: false });
    assert_eq!(
        AiSettings::resolve(Some("restricted"), None, None, Some(false)),
        AiSettings::Switches(Preset::Restricted.uses())
    );
}

#[test]
fn a_set_switch_overrides_the_preset_and_the_rest_follow_it() {
    let s = AiSettings::resolve(Some("restricted"), None, None, Some(true));
    assert_eq!(s, AiSettings::Switches(Uses { search: false, answers: false, training: true }));
    let s = AiSettings::resolve(None, Some(false), None, None);
    assert_eq!(s, AiSettings::Switches(Uses { search: false, answers: true, training: false }));
}

#[test]
fn default_switch_state_robots_txt() {
    let expected = "\
# This site's requests to the programs that read it. They are requests, not locks.
# Search engines: allowed. AI answers: allowed. AI training: asked not to.

User-agent: *
Content-Signal: search=yes, ai-input=yes, ai-train=no
Allow: /

# Asked not to train on this site
User-agent: GPTBot
User-agent: Google-Extended
User-agent: ClaudeBot
User-agent: CCBot
User-agent: Bytespider
User-agent: meta-externalagent
User-agent: Amazonbot
User-agent: Applebot-Extended
Disallow: /

Sitemap: https://example.com/sitemap.xml
";
    assert_eq!(switches(true, true, false).robots_txt(SITE), expected);
}

#[test]
fn all_off_asks_every_group_and_leaves_search_engines_free_to_read_noindex() {
    let expected = "\
# This site's requests to the programs that read it. They are requests, not locks.
# Search engines: asked not to. AI answers: asked not to. AI training: asked not to.

User-agent: *
Content-Signal: search=no, ai-input=no, ai-train=no
Allow: /

# Asked not to list this site in AI search. Search engines may still read pages, to see their noindex tag.
User-agent: OAI-SearchBot
User-agent: Claude-SearchBot
User-agent: PerplexityBot
Disallow: /

# Asked not to read pages to answer questions
User-agent: ChatGPT-User
User-agent: Claude-User
User-agent: Perplexity-User
Disallow: /

# Asked not to train on this site
User-agent: GPTBot
User-agent: Google-Extended
User-agent: ClaudeBot
User-agent: CCBot
User-agent: Bytespider
User-agent: meta-externalagent
User-agent: Amazonbot
User-agent: Applebot-Extended
Disallow: /
";
    let s = switches(false, false, false);
    assert_eq!(s.robots_txt(SITE), expected);
    assert!(!expected.contains("Googlebot") && !expected.contains("Bingbot"));
}

#[test]
fn answers_off_with_training_allowed_asks_google_extended_in_the_answers_group() {
    let robots = switches(true, false, true).robots_txt(SITE);
    assert!(robots.contains(
        "# Asked not to read pages to answer questions\nUser-agent: ChatGPT-User\nUser-agent: Claude-User\nUser-agent: Perplexity-User\nUser-agent: Google-Extended\nDisallow: /\n"
    ), "{robots}");
    assert!(!robots.contains("Asked not to train"), "{robots}");
}

#[test]
fn robots_meta_follows_search_and_training() {
    let meta = |s: AiSettings, draft: bool| s.robots_tag(draft);
    assert_eq!(meta(switches(true, true, true), false), None);
    assert_eq!(meta(switches(true, true, false), false).as_deref(), Some(r#"<meta name="robots" content="noai, noimageai">"#));
    assert_eq!(meta(switches(false, true, true), false).as_deref(), Some(r#"<meta name="robots" content="noindex">"#));
    assert_eq!(meta(switches(false, false, false), false).as_deref(), Some(r#"<meta name="robots" content="noindex, noai, noimageai">"#));
    // One tag, never two, when a draft is also on a site that refuses training.
    assert_eq!(meta(switches(true, true, false), true).as_deref(), Some(r#"<meta name="robots" content="noindex, noai, noimageai">"#));
}

#[test]
fn tdmrep_reserves_while_training_is_asked_not_to_and_stays_when_allowed() {
    assert_eq!(
        switches(true, true, false).tdmrep_json().as_deref(),
        Some("[\n  { \"location\": \"/\", \"tdm-reservation\": 1 }\n]\n")
    );
    assert_eq!(
        switches(true, true, true).tdmrep_json().as_deref(),
        Some("[\n  { \"location\": \"/\", \"tdm-reservation\": 0 }\n]\n")
    );
}

#[test]
fn llms_txt_follows_ai_answers_and_its_absence_is_declared() {
    assert!(!switches(true, false, false).publishes_llms_txt());
    let paths = |s: AiSettings| s.withdrawn().iter().map(|p| p.as_str().to_string()).collect::<Vec<_>>();
    assert_eq!(paths(switches(true, false, false)), ["llms.txt"]);
    assert!(switches(false, true, false).publishes_llms_txt());
    assert!(paths(switches(false, true, false)).is_empty());
    // With no switch set there is no tdmrep.json: removing the keys withdraws it.
    assert_eq!(paths(preset(None)), [".well-known/tdmrep.json"]);
    // A restricted site asked not to be used for answers, switches or not.
    assert!(!preset(Some("restricted")).publishes_llms_txt());
    assert!(preset(None).publishes_llms_txt() && preset(Some("unrestricted")).publishes_llms_txt());
}

#[test]
fn from_config_reads_the_same_keys_resolve_takes() {
    let cfg = crate::config::ConfigFile::parse("[site]\nai_policy = \"restricted\"\nai_training = true\n").unwrap();
    assert_eq!(AiSettings::from_config(Some(&cfg)), AiSettings::resolve(Some("restricted"), None, None, Some(true)));
    assert_eq!(AiSettings::from_config(None), AiSettings::default());
}

// ---- The publish gate reads a withdrawn llms.txt as the owner's act ----

fn removal_reasons(config: &str) -> Vec<(String, crate::build::manifest::change_set::RemovalReason)> {
    use crate::build::manifest::change_set::PublishedSnapshot;
    use crate::build::manifest::{backfill, published_record, HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;
    use crate::types::content::SiteHashes;

    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    std::fs::create_dir_all(vault.path().join(".moss")).unwrap();
    std::fs::write(vault.path().join(".moss/config.toml"), config).unwrap();
    let mut last = PendingManifest::new(SiteHashes::default());
    for path in ["llms.txt", ".well-known/tdmrep.json", "rss.xml"] {
        last.register(&ServedPath::from_cached(path).unwrap(), b"x", HashBucket::Files);
    }
    published_record::save(&mp, &PublishedSnapshot::from_sealed(&last.seal(), "moss:site-1", "2026-11-05T00:00:00Z".into())).unwrap();
    let now = PendingManifest::new(SiteHashes::default()).seal();
    let mut reasons: Vec<_> = backfill::removed_for_seal(&mp, &now).into_iter().map(|r| (r.path, r.reason)).collect();
    reasons.sort_by(|a, b| a.0.cmp(&b.0));
    reasons
}

#[test]
fn a_withdrawn_llms_txt_passes_the_publish_gate_and_nothing_else_does() {
    use crate::build::manifest::change_set::RemovalReason::{AuthorRemoved, Unexplained};
    let tdmrep = ".well-known/tdmrep.json".to_string();
    assert_eq!(
        removal_reasons("[site]\nai_answers = false\n"),
        [(tdmrep.clone(), Unexplained), ("llms.txt".to_string(), AuthorRemoved), ("rss.xml".to_string(), Unexplained)]
    );
    assert_eq!(
        removal_reasons("[site]\nai_answers = true\n"),
        [(tdmrep.clone(), Unexplained), ("llms.txt".to_string(), Unexplained), ("rss.xml".to_string(), Unexplained)]
    );
    // Removing every switch withdraws tdmrep.json, as the owner's choice.
    assert_eq!(
        removal_reasons("[site]\nlang = \"en\"\n"),
        [(tdmrep, AuthorRemoved), ("llms.txt".to_string(), Unexplained), ("rss.xml".to_string(), Unexplained)]
    );
}

/// The gate says which setting withdrew a file, not that its source was deleted.
#[test]
fn a_withdrawn_file_is_worded_by_the_setting_that_withdrew_it() {
    use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
    let line = |path: &str| crate::deploy::removal_gate::cause_line(&RemovedAddress { path: path.into(), reason: RemovalReason::AuthorRemoved, moved_to: None, source: None });
    assert_eq!(line("llms.txt"), "/llms.txt is gone because the site asks AI assistants not to use its pages.");
    assert_eq!(line(".well-known/tdmrep.json"), "/.well-known/tdmrep.json is gone because the site no longer sets its AI switches.");
}

#[test]
fn the_view_is_the_generators_own_output() {
    let s = switches(false, true, false);
    let v = s.view(SITE);
    assert!(v.switches_set && !v.search && v.answers && !v.training);
    assert_eq!(v.robots_txt, s.robots_txt(SITE));
    assert_eq!(v.robots_tag, s.robots_tag(false));
    assert!(!preset(None).view(SITE).switches_set);
}
