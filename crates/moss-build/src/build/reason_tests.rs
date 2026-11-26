use super::BuildReason::{self, Config, Manual, Open, Plugins, Publish, Setting, Watch};

#[test]
fn a_site_key_the_build_reads_names_itself_and_any_other_is_other() {
    assert_eq!(BuildReason::setting("color_scheme").to_string(), "setting:color_scheme");
    assert_eq!(BuildReason::setting("Jane's diary").to_string(), "setting:other");
}

#[test]
fn a_config_change_names_the_one_site_key_it_changed() {
    let before = "[site]\ncolor_scheme = \"light\"\nmath = true\n";
    assert_eq!(BuildReason::for_config_change(before, "[site]\ncolor_scheme = \"dark\"\nmath = true\n"), Some(Setting("color_scheme")));
    assert_eq!(BuildReason::for_config_change(before, "[site]\ncolor_scheme = \"light\"\nmath = true\nsearch = true\n"), Some(Setting("search")));
    assert_eq!(BuildReason::for_config_change(before, "[site]\ncolor_scheme = \"light\"\n"), Some(Setting("math")));
    assert_eq!(BuildReason::for_config_change(before, "[site]\ncolor_scheme = \"light\"\nmath = true\nmy_secret = 1\n"), Some(Setting("other")));
}

#[test]
fn any_other_config_change_is_config() {
    let before = "[site]\ncolor_scheme = \"light\"\nmath = true\n";
    assert_eq!(BuildReason::for_config_change(before, "[site]\ncolor_scheme = \"dark\"\nmath = false\n"), Some(Config));
    assert_eq!(BuildReason::for_config_change(before, &format!("{before}[deploy]\ntarget = \"x\"\n")), Some(Config));
    assert_eq!(BuildReason::for_config_change(before, "[site\n"), Some(Config));
    assert_eq!(BuildReason::for_config_change(before, "# a comment\n[site]\nmath = true\ncolor_scheme = \"light\"\n"), None);
}

#[test]
fn a_merged_build_keeps_the_reason_that_says_most() {
    assert_eq!(Watch.merge(Plugins), Plugins);
    assert_eq!(Publish.merge(Watch), Publish);
    assert_eq!(Setting("math").merge(Setting("math")), Setting("math"));
    assert_eq!(Setting("math").merge(Setting("search")), Config);
    assert_eq!(Config.merge(Setting("search")), Setting("search"));
    assert_eq!(Setting("math").merge(Config), Setting("math"));
    assert_eq!(Open.merge(Manual), Open);
}

#[test]
fn every_token_is_lowercase_schema_text() {
    let tokens: Vec<String> = BuildReason::every().iter().map(ToString::to_string).collect();
    for fixed in ["open", "watch", "manual", "publish", "config", "plugins", "site-address", "setting:color_scheme", "setting:other"] {
        assert!(tokens.iter().any(|t| t == fixed), "{fixed} missing from {tokens:?}");
    }
    assert!(tokens.iter().all(|t| t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-:".contains(&b))), "{tokens:?}");
}
