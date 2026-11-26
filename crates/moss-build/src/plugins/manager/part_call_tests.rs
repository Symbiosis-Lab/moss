//! A part's verbs run in the plugin's real code through the manager and the
//! engine, with no app.

use super::part_call::{DnsRecord, PartCall, PartRefusal};
use super::PluginManager;
use crate::plugins::adapter_host::AdapterHost;

pub(super) fn project(plugin: &str) -> (tempfile::TempDir, PluginManager) {
    let target = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
    std::fs::create_dir_all(target).unwrap();
    let container = tempfile::TempDir::new_in(target).unwrap();
    let project = container.path().join("project");
    let plugin_dir = project.join(".moss/plugins").join(plugin);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    for file in ["manifest.json", "main.bundle.js"] {
        let fixture = format!("{}/tests/fixtures/plugins/{plugin}/{file}", env!("CARGO_MANIFEST_DIR"));
        std::fs::copy(fixture, plugin_dir.join(file)).unwrap();
    }
    let reporter = std::sync::Arc::new(crate::build::ports::reporter::NullReporter);
    let manager = PluginManager::new(project.to_str().unwrap(), AdapterHost::headless(reporter)).unwrap();
    (container, manager)
}

fn set(content: &str) -> PartCall {
    let record = DnsRecord { name: "example.org".into(), kind: "A".into(), content: content.into(), ttl: 1, proxied: None };
    PartCall::DnsSet { zone: "z1".into(), name: "example.org".into(), kind: "A".into(), records: vec![record] }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_writes_at_once_each_reach_the_plugin() {
    let (_c, manager) = project("zone-host");
    let manager = std::sync::Arc::new(manager);
    let first = tokio::spawn({
        let manager = manager.clone();
        async move { manager.call_part("zone-host", &set("192.0.2.1")).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let second = manager.call_part("zone-host", &set("192.0.2.2")).await.unwrap();
    let first = first.await.unwrap().unwrap();
    assert_eq!((first[0]["content"].as_str(), second[0]["content"].as_str()), (Some("192.0.2.1"), Some("192.0.2.2")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_reads_of_one_verb_at_once_each_get_their_own_answer() {
    let (_c, manager) = project("zone-host");
    let manager = std::sync::Arc::new(manager);
    let zones = |name: &str| PartCall::DnsZones { name: Some(name.into()) };
    let first = tokio::spawn({
        let manager = manager.clone();
        async move { manager.call_part("zone-host", &zones("example.net")).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let second = manager.call_part("zone-host", &zones("example.com")).await.unwrap();
    let first = first.await.unwrap().unwrap();
    assert_eq!((first[0]["name"].as_str(), second[0]["name"].as_str()), (Some("example.net"), Some("example.com")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_returns_the_export_s_answer_and_a_refusal_names_the_part_s_need() {
    let (_c, manager) = project("zone-host");
    let zones = manager.call_part("zone-host", &PartCall::DnsZones { name: Some("example.net".into()) }).await.unwrap();
    assert_eq!(zones[0]["name"], "example.net");
    let refused = manager.call_part("zone-host", &PartCall::DnsRecords { zone: "z1".into() }).await.unwrap_err();
    assert_eq!(refused, PartRefusal::MissingPermission { needs: Some("Zone · Edit".into()) });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verb_of_a_part_the_plugin_does_not_serve_is_not_run() {
    let (_c, manager) = project("zone-host");
    let call = PartCall::AnalyticsSite { host: "example.org".into() };
    assert_eq!(manager.call_part("zone-host", &call).await.unwrap_err(), PartRefusal::NotServed);
    assert_eq!(manager.call_part("absent", &set("192.0.2.1")).await.unwrap_err(), PartRefusal::NotServed);
}
