//! Connecting analytics and reading it back, through the plugin's real code
//! and the secret store, with no app.

use super::VISITS_FILE;
use crate::plugins::manager::part_call::PartRefusal;
use crate::plugins::manager::part_call_tests::project;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_pasted_key_connects_the_site_and_reads_its_visits_back() {
    let (_c, manager) = project("visits-host");
    let site = std::path::PathBuf::from(&manager.project_path);
    std::fs::write(site.join(".moss/config.toml"), "[site]\nurl = \"https://example.org/\"\n").unwrap();

    assert_eq!(super::read_after_publish(&manager.project_path).await, None, "no analytics plugin named, nothing read");
    assert_eq!(manager.connect_analytics("visits-host", None).await, Err(PartRefusal::SignIn));
    let shape = manager.connect_analytics("visits-host", Some("short")).await;
    assert!(matches!(shape, Err(PartRefusal::WrongKey { .. })), "{shape:?}");

    let key = "k".repeat(40);
    assert_eq!(manager.connect_analytics("visits-host", Some(&key)).await.as_deref(), Ok("tag-example.org"));
    let config = std::fs::read_to_string(site.join(".moss/config.toml")).unwrap();
    assert!(config.contains("[services.analytics]") && config.contains("provider = \"visits-host\"") && config.contains("site_id = \"tag-example.org\""), "{config}");

    assert_eq!(manager.read_analytics().await, Ok(2));
    let file = site.join(".moss/data").join(VISITS_FILE);
    let first = super::read_visits(&file);
    assert_eq!(first.len(), 2);
    assert_eq!(manager.read_analytics().await, Ok(2));
    let second = super::read_visits(&file);
    assert_eq!(second.len(), 3, "the last day is read again, not kept twice: {second:?}");
    assert_eq!(second[0], first[0]);
    assert_eq!(super::read_after_publish(&manager.project_path).await.as_deref(), Some("Read 2 days of visits from visits-host"));
}

