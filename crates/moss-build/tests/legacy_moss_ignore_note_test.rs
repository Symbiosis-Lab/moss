//! A root `.gitignore` that excludes `.moss/*` without re-including the theme
//! and config leaves them out of version control; the build says so once and
//! names the lines that fix it. It never edits the author's `.gitignore`.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGE: (&str, &str) = ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n");

fn notes(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("belong in version control")).collect()
}

#[test]
fn the_legacy_pair_gets_a_note_with_the_replacement_lines() {
    let ignore = ".moss/*\n!.moss/site/\n";
    let site = Site::new(&[PAGE, (".gitignore", ignore)]);
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    for line in [".moss/*", "!.moss/theme/", "!.moss/config.toml", "!.moss/redirects.json"] {
        assert!(found[0].contains(line), "{}", found[0]);
    }
    assert!(site::problems().is_empty(), "a note is not a problem: {:?}", site::problems());
    assert_eq!(std::fs::read_to_string(site.root.join(".gitignore")).unwrap(), ignore);
}

#[test]
fn a_gitignore_that_reincludes_every_tracked_path_gets_no_note() {
    let all = ".moss/*\n!.moss/deploy/\n!.moss/plugins/\n!.moss/state.toml\n!.moss/redirects.json\n!.moss/config.toml\n!.moss/theme/\n!.moss/assets/\n!.moss/places.toml\n";
    let site = Site::new(&[PAGE, (".gitignore", all)]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}

#[test]
fn a_directory_rule_is_noted_even_with_reincludes() {
    let all = ".moss/\n!.moss/theme/\n!.moss/config.toml\n";
    let site = Site::new(&[PAGE, (".gitignore", all)]);
    assert_eq!(notes(&site).len(), 1, "{:?}", site.logged);
}

#[test]
fn theme_and_config_alone_leave_redirects_hidden() {
    let site = Site::new(&[PAGE, (".gitignore", ".moss/*\n!.moss/theme/\n!.moss/config.toml\n")]);
    assert_eq!(notes(&site).len(), 1, "{:?}", site.logged);
}

#[test]
fn a_site_without_a_gitignore_gets_no_note() {
    let site = Site::new(&[PAGE]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}
