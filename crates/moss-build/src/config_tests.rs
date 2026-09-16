use super::*;

/// A config as an advanced user actually writes one: section comments, a
/// trailing comment on a key, keys in the order that made sense to them
/// (`[build]` before `[site]`, `lang` after `search`), single quotes, an
/// array broken over lines, and a blank line where they wanted a breath.
///
/// Every test below reads THIS text, so a reader that only works on
/// moss-generated formatting fails here.
const HAND_WRITTEN: &str = "\
# 潮汐 — site configuration.
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

// The hand-editability ruling (ADR-059) is NOT asserted here, deliberately.
// A read-side "the text is unchanged afterwards" test in this crate cannot
// fail: `parse` takes `&str` and every accessor takes `&self`, so the borrow
// checker already forbids the mutation, and no edit to production code can
// turn such a test red. It was written, reviewed, and deleted for exactly
// that reason. The ruling is asserted where a writer actually exists —
// `src-tauri/tests/config_hand_editability_test.rs`, which reads a
// hand-written config through this reader and then compares the file
// byte-for-byte after a settings modal writes to it.

/// Open-CLI slice 3 (#1019): the one parse funnel migrates in memory, so a
/// host that never persists (moss-cli) still reads a legacy config at its
/// migrated meaning. Mirrors the parity fixture's v0 shape — pre-v1
/// `[features]`/`[analytics]` and no `schema_version` — whose analytics slot
/// and RSS footer are exactly what diverge if one host migrates and the
/// other does not (ADR-059's silent-defaults hazard).
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
