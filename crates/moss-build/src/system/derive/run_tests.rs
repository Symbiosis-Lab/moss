use super::*;
use crate::system::derive::test_support::{script, DERIVED};
use std::collections::BTreeMap;
use tempfile::TempDir;

fn input(dir: &TempDir) -> PathBuf {
    let p = dir.path().join("a.xyz");
    std::fs::write(&p, "data").unwrap();
    p
}

fn run(dir: &TempDir, body: &str) -> Result<DeriveOutcome, DeriveFailure> {
    let bin = script(dir.path(), "d.sh", body);
    run_deriver(&bin, &input(dir), Duration::from_secs(30))
}

#[test]
fn a_derived_answer_parses_and_unknown_facts_are_ignored() {
    let dir = TempDir::new().unwrap();
    let out = run(&dir, DERIVED).unwrap();
    assert_eq!(
        out,
        DeriveOutcome::Derived {
            fragments: BTreeMap::from([("text".into(), "hello".into())]),
            facts: crate::system::derive::DerivedFacts { title: Some("T".into()), ..Default::default() },
        }
    );
}

#[test]
fn the_request_carries_the_absolute_input_path() {
    let dir = TempDir::new().unwrap();
    // Echo the path back as a fragment: proves the request reached stdin intact.
    let body = r#"p=$(printf '%s' "$REQ" | sed 's/.*"path":"\([^"]*\)".*/\1/')
printf '{"kind":"derived","fragments":{"text":"%s"},"facts":{"title":null}}' "$p""#;
    let out = run(&dir, body).unwrap();
    let DeriveOutcome::Derived { fragments, .. } = out else { panic!("{out:?}") };
    assert_eq!(fragments["text"], input(&dir).to_str().unwrap());
}

#[test]
fn a_skip_answer_is_an_outcome_not_a_failure() {
    let dir = TempDir::new().unwrap();
    let out = run(&dir, r#"printf '{"kind":"skip","reason":"encrypted","message":"Needs a password."}'"#).unwrap();
    assert_eq!(
        out,
        DeriveOutcome::Skip { reason: "encrypted".into(), message: "Needs a password.".into() }
    );
}

#[test]
fn a_non_zero_exit_fails_with_the_last_stderr_line() {
    let dir = TempDir::new().unwrap();
    let err = run(&dir, "echo first >&2; echo 'boom: bad header' >&2; exit 3").unwrap_err();
    assert!(err.message.contains("boom: bad header"), "{err}");
    assert!(!err.message.contains("first"), "{err}");
}

#[test]
fn garbage_stdout_fails() {
    let dir = TempDir::new().unwrap();
    let err = run(&dir, "echo not json; echo warn >&2").unwrap_err();
    assert!(err.message.contains("did not print a derivation result"), "{err}");
    assert!(err.message.contains("warn"), "{err}");
}

#[test]
fn an_answer_of_an_unknown_kind_fails() {
    let dir = TempDir::new().unwrap();
    assert!(run(&dir, r#"printf '{"kind":"maybe"}'"#).is_err());
}

#[test]
fn undeclared_fragments_are_rejected_against_the_declaration() {
    let declared = BTreeMap::from([("text".to_string(), crate::plugins::contributions::derivation::FragmentType::Markdown)]);
    let ok = DeriveOutcome::Derived {
        fragments: BTreeMap::from([("text".into(), "x".into())]),
        facts: Default::default(),
    };
    assert!(ok.check_against(&declared).is_ok());
    let bad = DeriveOutcome::Derived {
        fragments: BTreeMap::from([("other".into(), "x".into())]),
        facts: Default::default(),
    };
    let err = bad.check_against(&declared).unwrap_err();
    assert!(err.message.contains("other"), "{err}");
}

#[test]
fn oversized_output_fails() {
    let dir = TempDir::new().unwrap();
    // 33 MiB of 'x' inside a string value.
    let body = r#"printf '{"kind":"skip","reason":"r","message":"'; head -c 34603008 /dev/zero | tr '\0' x; printf '"}'"#;
    let err = run(&dir, body).unwrap_err();
    assert!(err.message.contains("more than 32 MiB to stdout"), "{err}");
}

#[test]
fn a_deriver_that_floods_stderr_is_stopped_with_a_message() {
    let dir = TempDir::new().unwrap();
    let started = std::time::Instant::now();
    let err = run(&dir, "head -c 20000000 /dev/zero >&2; sleep 60").unwrap_err();
    assert!(err.message.contains("more than 1 MiB to stderr"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_timeout_names_itself_and_the_limit() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "slow.sh", "sleep 31338");
    let err = run_deriver(&bin, &input(&dir), Duration::from_secs(1)).unwrap_err();
    assert!(err.message.contains("timed out after 1s"), "{err}");
}

#[test]
fn a_planted_symlink_at_a_scratch_name_is_not_followed_or_deleted() {
    // The scratch name used to be pid + counter, so anyone could plant a
    // symlink there and have the run delete its target on drop.
    let victim = TempDir::new().unwrap();
    std::fs::write(victim.path().join("keep"), "x").unwrap();
    // Other tests share the counter, so cover every value this run could draw.
    let planted: Vec<_> = (0..256)
        .map(|n| std::env::temp_dir().join(format!("moss-derive-{}-{n}", std::process::id())))
        .collect();
    for link in &planted {
        let _ = std::fs::remove_file(link);
        std::os::unix::fs::symlink(victim.path(), link).unwrap();
    }
    let dir = TempDir::new().unwrap();
    run(&dir, DERIVED).unwrap();
    for link in &planted {
        let _ = std::fs::remove_file(link);
    }
    assert!(victim.path().join("keep").exists());
}

#[test]
fn the_child_sees_only_an_allowlisted_environment() {
    let dir = TempDir::new().unwrap();
    // A unique variable name no other test reads.
    std::env::set_var("MOSS_DERIVE_TEST_SECRET", "s3cret");
    let body = r#"printf '{"kind":"skip","reason":"r","message":"%s"}' "[$MOSS_DERIVE_TEST_SECRET][$HOME]""#;
    let DeriveOutcome::Skip { message, .. } = run(&dir, body).unwrap() else { panic!() };
    assert!(message.starts_with("[]["), "{message}");
    assert!(message.contains("moss-derive-"), "HOME is the scratch dir: {message}");
}

#[test]
fn a_run_past_its_deadline_is_stopped_with_everything_it_started() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "slow.sh", "sleep 31337 & sleep 31337; echo never");
    let started = std::time::Instant::now();
    let err = run_deriver(&bin, &input(&dir), Duration::from_millis(500)).unwrap_err();
    assert!(err.message.contains("timed out"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(30));
    let ps = std::process::Command::new("ps").args(["-axo", "command"]).output().unwrap();
    let ps = String::from_utf8_lossy(&ps.stdout);
    assert!(
        !ps.lines().any(|l| l.contains("sleep 31337")),
        "the child and its background sleeper are both gone:\n{ps}"
    );
}

#[test]
fn the_scratch_directory_is_gone_after_the_run() {
    let dir = TempDir::new().unwrap();
    let body = r#"printf '{"kind":"skip","reason":"r","message":"%s"}' "$TMPDIR""#;
    let DeriveOutcome::Skip { message, .. } = run(&dir, body).unwrap() else { panic!() };
    assert!(message.contains("moss-derive-"), "{message}");
    assert!(!Path::new(&message).exists());
}

#[test]
fn the_timeout_defaults_and_reads_a_positive_override() {
    assert_eq!(timeout_from(None), Duration::from_secs(600));
    assert_eq!(timeout_from(Some("7")), Duration::from_secs(7));
    assert_eq!(timeout_from(Some("0")), Duration::from_secs(600));
    assert_eq!(timeout_from(Some("soon")), Duration::from_secs(600));
}
