use super::*;
use std::collections::HashMap;

const MOSS: &str = "moss:site-1";
const ONION: &str = "onionpress:http://abc.onion/";

fn snap_for(target: &str) -> PublishedSnapshot {
    PublishedSnapshot {
        generation_id: "abc123".into(),
        target: target.into(),
        published_at: "2026-08-05T00:00:00Z".into(),
        sources: HashMap::from([("a.md".to_string(), "h-a".to_string())]),
        source_to_output: HashMap::from([("a.md".to_string(), "a/index.html".to_string())]),
        files: HashMap::from([("a/index.html".to_string(), "100644:deadbeef".to_string())]),
        uids: HashMap::from([("a.md".to_string(), "aabbccdd".to_string())]),
        triples: None,
    }
}

fn snap() -> PublishedSnapshot {
    snap_for(MOSS)
}

/// Write the pre-2026-08-17 single-slot record by hand — the layout every
/// already-published project has on disk at upgrade.
fn write_legacy(paths: &MossPaths, value: serde_json::Value) {
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    // allow:raw_write test fixture writing a legacy-layout record on purpose
    std::fs::write(legacy_path(paths), serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn save_then_load_round_trips_and_creates_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    assert_eq!(load_for(&paths, Some(MOSS)), None, "nothing published yet ⇒ degraded, not an error");

    save(&paths, &snap()).unwrap();

    assert!(path_for(&paths, MOSS).starts_with(paths.deploy_records_dir()));
    assert_eq!(load_for(&paths, Some(MOSS)), Some(snap()));
}

/// The headline of the keyed layout: two hosts, two baselines, neither
/// overwriting the other. Under the old single slot the second publish
/// destroyed the first host's record, so switching back reported an
/// unclassified change set about a site moss remembered perfectly well.
#[test]
fn publishing_to_another_host_leaves_this_hosts_record_intact() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());

    save(&paths, &snap_for(MOSS)).unwrap();
    save(&paths, &snap_for(ONION)).unwrap();

    assert_eq!(load_for(&paths, Some(MOSS)), Some(snap_for(MOSS)));
    assert_eq!(load_for(&paths, Some(ONION)), Some(snap_for(ONION)));
}

/// A record is keyed by the whole target, not by its method. Two moss-hosted
/// sites from one folder are as different as two hosts: diffing the new site
/// against the old one's tree under-reports, which is the direction the author
/// cannot correct from the UI.
#[test]
fn a_record_for_another_site_on_the_same_host_is_not_this_sites_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap_for("moss:site-a")).unwrap();

    assert!(load_for(&paths, Some("moss:site-a")).is_some(), "same target ⇒ diff against it");
    assert!(load_for(&paths, Some("moss:site-b")).is_none(), "different site ⇒ degrade, don't lie");
    assert!(
        load_for(&paths, Some("onionpress:http://x.onion/")).is_none(),
        "different host ⇒ degrade, don't lie"
    );
}

/// A crash mid-write, or any other corruption, degrades the change set rather
/// than failing the build or — worse — deserializing into a wrong diff.
#[test]
fn unreadable_record_loads_as_none() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap()).unwrap();
    // allow:raw_write test fixture corrupting a record on purpose
    std::fs::write(path_for(&paths, MOSS), b"{\"generation_id\": tru").unwrap();

    assert_eq!(load_for(&paths, Some(MOSS)), None);
}

/// The write leaves no temp file behind, so a later `read_dir` of the records
/// directory sees exactly one record per target.
#[test]
fn save_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap()).unwrap();
    save(&paths, &snap()).unwrap();

    let entries: Vec<_> = std::fs::read_dir(paths.deploy_records_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries.len(), 1, "one target ⇒ one file, got {entries:?}");
}

// ── Migration off the single slot ───────────────────────────────────────
//
// Every project published before the keyed layout has exactly one record, at
// `.moss/deploy/last-published.json`. Refusing to read it would hand each of
// them one unclassified change set on upgrade — "no machine record" about a
// publish moss remembers perfectly well.

#[test]
fn a_legacy_single_slot_record_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    write_legacy(&paths, serde_json::to_value(snap_for(MOSS)).unwrap());

    assert_eq!(load_for(&paths, Some(MOSS)), Some(snap_for(MOSS)));
}

/// A record written before the `site_id` → `target` rename carries no host
/// prefix, and only moss hosting ever wrote one — so it answers for that site
/// under moss hosting and for nothing else.
#[test]
fn a_pre_rename_legacy_record_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    write_legacy(
        &paths,
        serde_json::json!({
            "generation_id": "abc123",
            "site_id": "site-1",
            "published_at": "2026-08-05T00:00:00Z",
            "sources": { "a.md": "h-a" },
            "source_to_output": { "a.md": "a/index.html" },
            "files": { "a/index.html": "100644:deadbeef" },
        }),
    );

    // The alias carries the old value through verbatim — unprefixed, which is
    // what marks it as moss hosting's (nothing else ever wrote one). It carries
    // no note IDs, because nothing recorded them until moss#1079; that loads as
    // an empty map here, and `live_baseline` is what refuses to read absence
    // into it.
    let loaded = load_for(&paths, Some(MOSS)).expect("a pre-rename record must still load");
    assert_eq!(
        loaded,
        PublishedSnapshot { target: "site-1".into(), uids: HashMap::new(), ..snap() }
    );
    assert!(load_for(&paths, Some(ONION)).is_none(), "it can only answer for moss hosting");
}

/// The legacy slot is read once and then retired. Leaving it on disk would let
/// a stale baseline outlive the keyed record that replaced it.
#[test]
fn saving_retires_the_legacy_slot() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    write_legacy(&paths, serde_json::to_value(snap_for(MOSS)).unwrap());

    save(&paths, &snap_for(ONION)).unwrap();

    assert!(!legacy_path(&paths).exists(), "the single slot is retired on first save");
    assert_eq!(load_for(&paths, Some(ONION)), Some(snap_for(ONION)));
    assert!(load_for(&paths, Some(MOSS)).is_none(), "and its record does not survive as a lie");
}

/// A plugin record written under the pre-slot `<method>:<url>` key still
/// answers when queried by today's bare-method slot — the upgrade must not
/// hand a published site a degraded change set. Retires with `legacy_path`.
#[test]
fn a_pre_slot_plugin_record_answers_for_its_bare_method_slot() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap_for(ONION)).unwrap();

    assert_eq!(load_for(&paths, Some("onionpress")), Some(snap_for(ONION)));
    assert!(load_for(&paths, Some("ipfs")).is_none(), "another method's slot stays degraded");
}

// ── No configured target ────────────────────────────────────────────────

/// A folder with no host configured yet cannot contradict a record, so the one
/// record on disk answers for it — that is how a first build after an upgrade
/// still classifies.
#[test]
fn with_no_configured_target_the_sole_record_answers() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap_for(MOSS)).unwrap();

    assert_eq!(load_for(&paths, None), Some(snap_for(MOSS)));
}

/// With two of them there is no sole record to fall back on, and picking one
/// would be a guess about which host the next publish goes to.
#[test]
fn with_no_configured_target_and_two_records_nothing_answers() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap_for(MOSS)).unwrap();
    save(&paths, &snap_for(ONION)).unwrap();

    assert_eq!(load_for(&paths, None), None);
}

// ── Key derivation ──────────────────────────────────────────────────────

/// The filename is an index, never the authority: `load_for` re-checks the
/// `target` inside the file. So a key collision degrades the change set
/// instead of diffing against another host's tree.
#[test]
fn a_record_found_under_the_key_must_still_name_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());
    save(&paths, &snap_for(MOSS)).unwrap();
    // Plant the wrong host's record under this host's key.
    let bytes = serde_json::to_vec(&snap_for(ONION)).unwrap();
    // allow:raw_write test fixture planting a mis-keyed record on purpose
    std::fs::write(path_for(&paths, MOSS), bytes).unwrap();

    assert_eq!(load_for(&paths, Some(MOSS)), None);
}

/// A target is an arbitrary string — an onion URL carries `:` and `/`, which
/// cannot go into a filename. The key stays one flat, readable file name.
#[test]
fn the_key_is_a_flat_filename_that_names_its_host() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());

    let key = path_for(&paths, ONION);
    let name = key.file_name().unwrap().to_string_lossy().to_string();

    assert_eq!(key.parent().unwrap(), paths.deploy_records_dir(), "one flat directory");
    assert!(name.starts_with("onionpress-"), "the host is legible on disk, got {name}");
    assert!(name.ends_with(".json"));
    assert!(!name[..name.len() - 5].contains('.'), "no path traversal, got {name}");
}

#[test]
fn the_key_is_stable_and_distinct_per_target() {
    let dir = tempfile::tempdir().unwrap();
    let paths = MossPaths::new(dir.path());

    assert_eq!(path_for(&paths, MOSS), path_for(&paths, MOSS), "same target ⇒ same file");
    assert_ne!(path_for(&paths, MOSS), path_for(&paths, "moss:site-2"));
    assert_ne!(path_for(&paths, MOSS), path_for(&paths, ONION));
}
