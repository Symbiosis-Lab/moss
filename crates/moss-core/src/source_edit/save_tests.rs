use super::*;
use crate::page::content_hash;
use serde_json::json;

#[test]
fn identical_disk_is_saved_even_with_a_stale_hash() {
    assert_eq!(decide_save(Some(b"body"), Some("stale"), &json!({}), "body").unwrap(), SaveDecision::AlreadySaved);
}

#[test]
fn changed_disk_returns_the_parsed_conflict() {
    let decision = decide_save(Some(b"other writer"), Some(&content_hash(b"before")), &json!({}), "mine").unwrap();
    let SaveDecision::Conflict { disk } = decision else { panic!("expected conflict") };
    assert_eq!(disk.body, "other writer");
    assert_eq!(disk.hash, content_hash(b"other writer"));
}

#[test]
fn matching_hash_missing_hash_and_new_file_allow_writes() {
    for (disk, hash) in [(Some(b"old".as_slice()), Some(content_hash(b"old"))), (Some(b"old".as_slice()), None), (None, None)] {
        assert_eq!(decide_save(disk, hash.as_deref(), &json!({}), "new").unwrap(), SaveDecision::Write { bytes: b"new".to_vec() });
    }
}

#[test]
fn formatting_and_union_coercion_survive_the_decision() {
    let disk = "---\n# card\ntitle: \"Before\"\nuid: 123\nseries: 5\n---\nold";
    let decision = decide_save(Some(disk.as_bytes()), None, &json!({"title":"After", "uid":123,"series":5}), "new").unwrap();
    let SaveDecision::Write { bytes } = decision else { panic!("expected write") };
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("# card"));
    let parsed = parse_frontmatter_of(text).unwrap();
    assert_eq!(parsed.frontmatter["uid"], "123");
    assert_eq!(parsed.frontmatter["series"], false);
    assert_eq!(parsed.body, "new");
}

#[test]
fn invalid_utf8_and_non_object_frontmatter_are_errors() {
    assert!(decide_save(Some(&[255]), None, &json!({}), "body").is_err());
    assert!(decide_save(None, None, &json!([]), "body").is_err());
}

#[test]
fn missing_disk_with_an_existing_revision_is_gone() {
    assert_eq!(decide_save(None, Some(&content_hash(b"old")), &json!({}), "new").unwrap(), SaveDecision::Gone);
    assert_eq!(decide_save(None, None, &json!({}), "new").unwrap(), SaveDecision::Write { bytes: b"new".to_vec() });
}
