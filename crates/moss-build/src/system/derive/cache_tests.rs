//! "Did it run again?" is answered by swapping the binary for one that fails:
//! a call that still succeeds never reached it.

use super::*;
use crate::system::derive::test_support::{entry, script, DERIVED};
use std::path::PathBuf;
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    site: MossPaths,
    input: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let site = MossPaths::new(&dir.path().join("site"));
    let input = dir.path().join("a.xyz");
    std::fs::write(&input, "content").unwrap();
    Fixture { dir, site, input }
}

impl Fixture {
    fn bin(&self, body: &str) -> PathBuf {
        script(self.dir.path(), "d.sh", body)
    }

    /// Make the binary unrunnable without touching its content, so a call that
    /// still succeeds was answered by the cache.
    fn disable(&self) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.dir.path().join("d.sh"), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

const BROKEN: &str = "echo 'must not run' >&2; exit 9";
const SKIP: &str = r#"printf '{"kind":"skip","reason":"empty","message":"Nothing here."}'"#;

#[test]
fn a_second_derive_of_the_same_content_does_not_run_the_binary() {
    let f = fixture();
    let bin = f.bin(DERIVED);
    let e = entry(&bin, "1.0.0");

    let first = derive_cached(&f.site, &e, &f.input).unwrap();
    f.disable();
    let second = derive_cached(&f.site, &e, &f.input).expect("served from the cache");
    assert_eq!(second, first);
}

#[test]
fn lookup_cached_reads_without_running_and_misses_before_a_derive() {
    let f = fixture();
    let e = entry(&f.bin(DERIVED), "1.0.0");
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None);
    let derived = derive_cached(&f.site, &e, &f.input).unwrap();
    f.disable();
    assert_eq!(lookup_cached(&f.site, &e, &f.input), Some(derived));
}

#[test]
fn a_skip_is_cached() {
    let f = fixture();
    let e = entry(&f.bin(SKIP), "1.0.0");
    derive_cached(&f.site, &e, &f.input).unwrap();
    f.disable();
    let again = derive_cached(&f.site, &e, &f.input).unwrap();
    assert!(matches!(again, DeriveOutcome::Skip { .. }));
}

#[test]
fn a_failure_is_not_cached() {
    let f = fixture();
    let e = entry(&f.bin(BROKEN), "1.0.0");
    assert!(derive_cached(&f.site, &e, &f.input).is_err());
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None);
    f.bin(DERIVED);
    derive_cached(&f.site, &e, &f.input).expect("the retry runs and succeeds");
}

#[test]
fn an_undeclared_fragment_is_a_failure_and_is_not_cached() {
    let f = fixture();
    let e = entry(&f.bin(r#"printf '{"kind":"derived","fragments":{"rogue":"x"},"facts":{}}'"#), "1.0.0");
    let err = derive_cached(&f.site, &e, &f.input).unwrap_err();
    assert!(err.message.contains("rogue"), "{err}");
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None);
}

#[test]
fn a_new_plugin_version_misses_and_a_changed_file_misses() {
    let f = fixture();
    let e1 = entry(&f.bin(DERIVED), "1.0.0");
    derive_cached(&f.site, &e1, &f.input).unwrap();

    f.disable();
    let e2 = entry(&f.dir.path().join("d.sh"), "1.1.0");
    assert!(derive_cached(&f.site, &e2, &f.input).is_err(), "version 1.1.0 must re-run");

    std::fs::write(&f.input, "edited").unwrap();
    assert!(derive_cached(&f.site, &e1, &f.input).is_err(), "new content must re-run");
}

#[test]
fn the_previous_result_is_found_across_versions_for_the_same_bytes_only() {
    let f = fixture();
    let e1 = entry(&f.bin(DERIVED), "1.0.0");
    let e2 = entry(&f.dir.path().join("d.sh"), "1.1.0");
    assert_eq!(lookup_previous(&f.site, &e2, &f.input), None, "never derived");
    derive_cached(&f.site, &e1, &f.input).unwrap();
    assert_eq!(lookup_cached(&f.site, &e2, &f.input), None);
    assert!(lookup_previous(&f.site, &e2, &f.input).is_some());
    std::fs::write(&f.input, "edited").unwrap();
    assert_eq!(lookup_previous(&f.site, &e2, &f.input), None, "other bytes");
}

#[test]
fn another_derivation_does_not_see_this_ones_result() {
    let f = fixture();
    let e = entry(&f.bin(DERIVED), "1.0.0");
    derive_cached(&f.site, &e, &f.input).unwrap();
    let mut other = e.clone();
    other.derivation_id = "other".into();
    assert_eq!(lookup_cached(&f.site, &other, &f.input), None);
}

#[test]
fn rebuilding_a_local_binary_or_changing_the_fragments_misses_without_a_version_bump() {
    let f = fixture();
    let e = entry(&f.bin(DERIVED), "1.0.0");
    let first = derive_cached(&f.site, &e, &f.input).unwrap();

    // Same path, same version, new content: the developer rebuilt the binary.
    f.bin(r#"printf '{"kind":"derived","fragments":{"text":"rebuilt"},"facts":{}}'"#);
    let rebuilt = derive_cached(&f.site, &e, &f.input).unwrap();
    assert_ne!(rebuilt, first);

    // Same content, same version, changed fragments.
    f.disable();
    let mut refragmented = e.clone();
    refragmented.contribution.fragments.clear();
    assert_eq!(lookup_cached(&f.site, &refragmented, &f.input), None);
    assert!(lookup_cached(&f.site, &e, &f.input).is_some(), "the unchanged declaration still hits");
}

#[test]
fn a_result_the_cache_cannot_keep_is_remembered_only_by_background_reads() {
    let f = fixture();
    let e = entry(&f.bin(DERIVED), "1.0.0");
    // A file where the object store's directory should be: nothing can be stored.
    for objects in [f.site.cache_objects(), f.site.cache_local_objects()] {
        std::fs::create_dir_all(objects.parent().unwrap()).unwrap();
        std::fs::write(&objects, "not a directory").unwrap();
    }

    derive_cached(&f.site, &e, &f.input).unwrap();
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None, "nothing was stored");
    assert_eq!(remembered(&e, &f.input), None, "a build that waits for text keeps no memory");

    let background = derive_remembering(&f.site, &e, &f.input).unwrap();
    assert_eq!(remembered(&e, &f.input), Some(Ok(background)));

    let mut edited = e.clone();
    edited.plugin_version = "2.0.0".into();
    assert_eq!(remembered(&edited, &f.input), None, "the memory is keyed like the cache");
}

#[test]
fn a_failed_run_is_remembered_by_background_reads_and_noted_for_status() {
    let f = fixture();
    let e = entry(&f.bin(BROKEN), "1.0.0");
    let failure = derive_remembering(&f.site, &e, &f.input).unwrap_err();
    assert_eq!(remembered(&e, &f.input), Some(Err(failure.clone())));
    assert_eq!(lookup_failure(&f.site, &e, &f.input), Some(failure.message));
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None, "a failure never stops a retry");
}

#[test]
fn every_question_about_one_file_hashes_it_once() {
    use super::binary::COMPUTED as HASH_READS;
    let f = fixture();
    let e = entry(&f.bin(DERIVED), "1.0.0");
    // The binary's own hash is part of the key; take it out of the count.
    let _ = params(&e);
    HASH_READS.with(|count| count.set(0));
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None);
    assert_eq!(lookup_failure(&f.site, &e, &f.input), None);
    assert!(remembered(&e, &f.input).is_none());
    derive_cached(&f.site, &e, &f.input).unwrap();
    assert!(lookup_cached(&f.site, &e, &f.input).is_some());
    assert_eq!(HASH_READS.with(|count| count.get()), 1, "one read of the source for all five questions");

    std::fs::write(&f.input, "changed content").unwrap();
    assert_eq!(lookup_cached(&f.site, &e, &f.input), None);
    assert_eq!(HASH_READS.with(|count| count.get()), 2, "an edited file is hashed again");
}

const ANSWER: &str = r#"{"kind":"derived","fragments":{"text":"hello"},"facts":{"title":"T","description":"D","extra":1}}"#;

#[test]
fn an_entry_written_as_the_deriver_printed_it_yields_every_fact_the_reader_knows() {
    let f = fixture();
    let e = entry(&f.bin(BROKEN), "1.0.0");
    let oid = binary::content_hash(&f.input).unwrap();
    store_bytes(&f.site, transform_name(&e), &e, &oid, 7, ANSWER.as_bytes()).unwrap();
    match lookup_cached(&f.site, &e, &f.input) {
        Some(DeriveOutcome::Derived { facts, .. }) => {
            assert_eq!((facts.title.as_deref(), facts.description.as_deref()), (Some("T"), Some("D")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_cache_keeps_the_bytes_the_deriver_printed_not_a_re_serialization() {
    let f = fixture();
    // A trailing newline and a fact this moss does not know: both are the deriver's own.
    let e = entry(&f.bin(&format!("printf '%s\\n' '{ANSWER}'")), "1.0.0");
    derive_cached(&f.site, &e, &f.input).unwrap();
    let oid = binary::content_hash(&f.input).unwrap();
    let stored = lookup_bytes(&f.site, &transform_name(&e), &e, &oid).expect("stored");
    assert_eq!(String::from_utf8(stored).unwrap(), ANSWER);
}

#[cfg(target_os = "macos")]
#[test]
fn a_derivation_keeps_files_in_its_plugins_cache_directory_across_runs() {
    let f = fixture();
    let stacks = f.dir.path().join("stacks");
    std::env::set_var("MOSS_STACKS_ROOT", &stacks);
    // The second run reads what the first wrote, and says so in its text.
    let bin = f.bin(
        r#"if [ -f "$MOSS_CACHE_DIR/seen" ]; then t=cached; else t=fresh; echo x > "$MOSS_CACHE_DIR/seen"; fi
printf '{"kind":"derived","fragments":{"text":"%s"},"facts":{}}' "$t""#,
    );
    let e = entry(&bin, "1.0.0");
    let text = |o: DeriveOutcome| match o {
        DeriveOutcome::Derived { fragments, .. } => fragments["text"].clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(text(run_pinned(&e, &f.input).unwrap().outcome), "fresh");
    assert_eq!(text(run_pinned(&e, &f.input).unwrap().outcome), "cached");
    assert!(stacks.join("derive/docs/.cache/seen").exists());
    std::env::remove_var("MOSS_STACKS_ROOT");
}
