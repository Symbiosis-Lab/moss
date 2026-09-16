use std::fs;

use super::*;

#[test]
fn site_key_falls_back_to_a_path_hash_before_any_identity_exists() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();

    let (key, source) = site_key(&vault);
    assert_eq!(source, KeySource::Path);
    assert_eq!(key.len(), 16);
    assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));

    let (key_again, _) = site_key(&vault);
    assert_eq!(key, key_again, "the same vault must resolve to the same key");
}

#[test]
fn site_key_prefers_the_vault_identity_once_one_exists() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let identity = crate::identity::Identity::generate().unwrap();
    identity.save(&vault).unwrap();

    let (key, source) = site_key(&vault);
    assert_eq!(source, KeySource::Identity);
    assert_eq!(key, &identity.pubkey[..16]);
}

#[test]
fn list_records_is_empty_before_any_publish() {
    let dir = tempfile::tempdir().unwrap();

    assert!(list_records(&dir.path().join("history")).is_empty());
}

#[test]
fn load_record_returns_none_for_an_unknown_id() {
    let dir = tempfile::tempdir().unwrap();

    assert!(load_record(&dir.path().join("history"), "no-such-id").is_none());
}

#[test]
fn is_live_matches_only_the_generation_id_the_baseline_names() {
    use super::super::record::{PublishRecord, Trigger};
    use std::collections::BTreeMap;

    let record = PublishRecord {
        version: 1,
        published_at: "2026-09-10T08:12:33Z".to_string(),
        target: "moss:site-1".to_string(),
        generation_id: "gen-live".to_string(),
        git_head: None,
        trigger: Trigger::Publish,
        label: None,
        entries: BTreeMap::new(),
    };

    assert!(is_live(&record, Some("gen-live")));
    assert!(!is_live(&record, Some("gen-other")));
    assert!(!is_live(&record, None), "no deploy baseline at all must never read as live");
}

#[test]
fn a_newer_but_unpublished_record_is_never_live() {
    // "Live" is not recency: a manual save taken after the last real publish
    // must not wear the badge just because it is the newest record.
    use super::super::record::{PublishRecord, Trigger};
    use std::collections::BTreeMap;

    let manual_after_publish = PublishRecord {
        version: 1,
        published_at: "2026-09-11T00:00:00Z".to_string(),
        target: "moss:site-1".to_string(),
        generation_id: "gen-manual".to_string(),
        git_head: None,
        trigger: Trigger::Manual,
        label: Some("Before rewriting the intro".to_string()),
        entries: BTreeMap::new(),
    };

    assert!(!is_live(&manual_after_publish, Some("gen-live-publish")));
}
