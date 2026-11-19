//! The running app's data roots follow the identifier its binary seeds. Its
//! own test binary, because the seed is set once per process.

use moss_build::infra::app_data::{app_config_path_early, seed_app_identifier, AppRoots, SHIPPED_IDENTIFIER};

#[test]
fn the_running_roots_are_the_shipped_apps_until_a_binary_seeds_its_own() {
    assert_eq!(AppRoots::running(), AppRoots::for_identifier(SHIPPED_IDENTIFIER), "the command line is the shipped app");

    seed_app_identifier("host.moss.publisher.wt-test");

    assert_eq!(AppRoots::running(), AppRoots::for_identifier("host.moss.publisher.wt-test"));
    let settings = app_config_path_early().unwrap();
    assert!(settings.ends_with("host.moss.publisher.wt-test/app-config.json"), "a local build reads its own settings: {}", settings.display());
}
