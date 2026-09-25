//! The one property worth testing here is the *direction*: `?` can only ever
//! produce `Fatal`, and `Deferred` and `Discarded` can only ever come from a
//! classified `io::Error`. Getting that backwards turns a real build failure
//! into an unclearable waiting screen, or into a silent second build.

use super::*;
use std::io;

/// The vault every path below sits in. Its mount shape is not the subject
/// here — `scope`'s relocation table crosses `disposition` with all of them.
const ROOT: &str = "/Users/x/Vault";

fn root() -> &'static std::path::Path {
    std::path::Path::new(ROOT)
}

#[test]
fn a_question_mark_on_a_string_error_is_fatal() {
    fn inner() -> Result<(), BuildStopped> {
        let r: Result<(), String> = Err("invalid frontmatter in posts/hello.md".into());
        r?;
        Ok(())
    }
    let stopped = inner().unwrap_err();
    assert!(!stopped.is_deferred(), "`?` must never produce a deferred stop");
    assert!(!stopped.is_discarded(), "nor a discarded one — that would re-run the build");
    assert_eq!(stopped.message(), "invalid frontmatter in posts/hello.md");
}

#[test]
fn a_question_mark_on_an_io_error_is_fatal() {
    fn inner() -> Result<Vec<u8>, BuildStopped> {
        Ok(std::fs::read("/definitely/not/here/moss-outcome-probe.bin")?)
    }
    assert!(!inner().unwrap_err().is_deferred());
}

/// An ordinary I/O failure stays fatal even when it goes through the
/// classifier — waiting for a download does not create a missing directory.
#[test]
fn an_ordinary_io_failure_classifies_as_fatal() {
    let path = std::path::Path::new("/definitely/not/here/moss-outcome-probe.bin");
    let e = std::fs::read(path).unwrap_err();
    let stopped = io_stop(root(), "read OG card for hashing", path, e);
    assert!(!stopped.is_deferred());
    assert!(
        stopped.message().starts_with("Failed to read OG card for hashing /definitely/not/here/"),
        "the message keeps the context, the path and the OS error: {}",
        stopped.message()
    );
}

/// `EDEADLK` is the fail-fast policy (`platform::macos::iopolicy`) reporting
/// that a file's bytes are still in the cloud — the case that must defer.
///
/// macOS-only on purpose, and asserted as a literal rather than against the
/// classifier: comparing `io_stop`'s answer to the function `io_stop` calls
/// would pass no matter which way either one went. On Linux there is no File
/// Provider, `EDEADLK` means an actual deadlock, and deferring on it would hang
/// the user in a waiting screen for a download that is never coming — so the
/// negative case below is the assertion that matters on this box.
#[test]
#[cfg(target_os = "macos")]
fn edeadlk_defers() {
    let path = std::path::Path::new("/tmp/moss-outcome-edeadlk-probe.png");
    let e = io::Error::from_raw_os_error(libc::EDEADLK);
    assert!(io_stop(root(), "read OG card for hashing", path, e).is_deferred());
}

#[test]
#[cfg(all(unix, not(target_os = "macos")))] // libc::EDEADLK is a unix errno; Windows has no File Provider and no EDEADLK
fn edeadlk_is_fatal_where_there_is_no_file_provider() {
    let path = std::path::Path::new("/tmp/moss-outcome-edeadlk-probe.png");
    let e = io::Error::from_raw_os_error(libc::EDEADLK);
    assert!(
        !io_stop(root(), "read OG card for hashing", path, e).is_deferred(),
        "no File Provider here: EDEADLK is a real deadlock, and waiting for a \
         download that will never arrive strands the user in the gate"
    );
}

/// The three dispositions, which are the whole of `io_stop`'s policy. Asserted
/// on the predicate rather than through a real eviction, because provoking one
/// needs a File Provider.
///
/// The question each row answers is **"would waiting actually end?"**, not "is
/// this file important" — a `Deferred` stop raises a screen only a later
/// successful build can lower.
#[test]
fn every_path_gets_the_disposition_whose_wait_can_actually_end() {
    // The stage and the CAS: everything the next build either rewrites from
    // source or re-derives. Extension-independent — an OG card, a copied
    // notebook asset and a cache blob are all regenerable, and the `.html`
    // special case that used to be here left them reported instead.
    for p in [
        "/Users/x/Vault/.moss/build.nosync/staging/index.html",
        "/Users/x/Vault/.moss/build.nosync/staging/posts/hello/index.html",
        "/Users/x/Vault/.moss/build.nosync/staging/_moss/og/abc123.png",
        "/Users/x/Vault/.moss/build.nosync/staging/jupyter/jupyter-lite.json",
        "/Users/x/Vault/.moss/cache/objects/ab/cd1234",
    ] {
        assert_eq!(disposition(root(), std::path::Path::new(p)), Disposition::Discard, "{p}");
    }

    // Watched inputs: the arrival trips the watcher, so the gate it raises can
    // come back down.
    for p in [
        "/Users/x/Vault/posts/hello.md",
        "/Users/x/Vault/images/cover.jpg",
        "/Users/x/Vault/.moss/config.toml",
        "/Users/x/Vault/.moss/theme/custom.css",
        "/Users/x/Vault/.moss/assets/logo.svg",
        // `data/social` is watched; the rest of `data/` is not.
        "/Users/x/Vault/.moss/data/social/matters.json",
        // Real build inputs whose extensions the watcher used to ignore.
        // Their arrival scheduled no rebuild (the round-3 defect), so they
        // were `Report`; now the watcher's allowlist covers everything the
        // pipeline consumes, and delegation picks that up for free.
        "/Users/x/Vault/clips/demo.mov",
        "/Users/x/Vault/analysis.ipynb",
    ] {
        assert_eq!(disposition(root(), std::path::Path::new(p)), Disposition::Wait, "{p}");
    }

    // Neither regenerable-in-place nor watched. Deferring on one of these would
    // raise a screen nothing can lower; discarding one would destroy the
    // generation the preview server is serving right now.
    for p in [
        "/Users/x/Vault/.moss/build.nosync/generations/ab12/index.html",
        "/Users/x/Vault/.moss/data/redirects.json",
        "/Users/x/Vault/.moss/plugins/github/main.js",
        // A file whose extension nothing consumes. Its arrival schedules no
        // rebuild, so a waiting screen over it would never lift — the shape of
        // the round-3 defect, which `.mov`/`.ipynb` (now watched) used to pin.
        "/Users/x/Vault/report.docx",
    ] {
        assert_eq!(disposition(root(), std::path::Path::new(p)), Disposition::Report, "{p}");
    }
}

/// A vault whose own directory is named `.mossy` is not moss's `.moss`: its
/// `build/staging/` is the user's own folder. A non-input there must be
/// reported, while an HTML input must wait for the watcher. A substring gate
/// would mistake `.mossy` for moss's own staging and discard either one.
#[test]
fn a_vault_named_dot_mossy_has_no_moss_staging() {
    let root = std::path::Path::new("/Users/x/.mossy");
    let staged = std::path::Path::new("/Users/x/.mossy/build/staging/report.docx");
    assert_eq!(disposition(root, staged), Disposition::Report);
    let watched = std::path::Path::new("/Users/x/.mossy/build/staging/index.html");
    assert_eq!(disposition(root, watched), Disposition::Wait);
}

/// `disposition` is a vault-path predicate, so it is held to the same
/// relocation invariant as the watcher predicates it delegates to: a
/// Google shared-drive vault used to classify EVERY dataless read as `Report`,
/// so the user saw a raw OS error where the waiting screen belonged.
#[test]
fn a_relocated_vault_gets_the_same_disposition() {
    use crate::build::watch::scope::{mount_join, VAULT_MOUNTS};
    for (shape, mount) in VAULT_MOUNTS {
        let root = std::path::Path::new(mount);
        for rel in ["posts/hello.md", "images/cover.jpg", ".moss/config.toml", "clips/demo.mov"] {
            assert_eq!(
                disposition(root, &mount_join(mount, rel)),
                Disposition::Wait,
                "`{rel}` under {shape}: its arrival trips the watcher, so the gate can lift"
            );
        }
        assert_eq!(
            disposition(root, &mount_join(mount, ".moss/build.nosync/staging/index.html")),
            Disposition::Discard,
            "a staged page under {shape}"
        );
        for rel in ["report.docx", ".moss/data/redirects.json"] {
            assert_eq!(
                disposition(root, &mount_join(mount, rel)),
                Disposition::Report,
                "`{rel}` under {shape}: no arrival here schedules a rebuild"
            );
        }
    }
}

/// The discard is gated on the eviction classifier, not on the path alone. An
/// ordinary failure must never delete build output — that would turn a
/// transient permission problem into a lost generation.
#[test]
fn a_present_output_file_is_not_discarded() {
    let dir = tempfile::tempdir().unwrap();
    let staged = dir.path().join(".moss/build.nosync/staging/index.html");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, b"<html>").unwrap();

    let e = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
    let stopped = io_stop(dir.path(), "read staged HTML", &staged, e);

    assert!(!stopped.is_deferred(), "a permission error is not a cloud wait");
    assert!(staged.is_file(), "an ordinary failure must never delete build output");
}

/// `with_context` is the only sanctioned way to add context to a stop, and its
/// whole point is that it does NOT do what `map_err(|e| format!("…: {e}"))`
/// would do. If it ever dropped the flag, the gate would go silent again at the
/// one live call site that uses it (`pipeline::apply_slots_to_stage_and_manifest`).
#[test]
fn with_context_keeps_the_verdict() {
    let deferred = BuildStopped::deferred("still downloading").with_context("Slot injection");
    assert!(deferred.is_deferred(), "context must not downgrade a deferred stop");
    assert_eq!(deferred.message(), "Slot injection: still downloading");

    let fatal = BuildStopped::from("plugin returned invalid JSON").with_context("Slot injection");
    assert!(!fatal.is_deferred(), "and must not promote a fatal one");
}

/// Both verdicts are raised deep inside `build_inner` and read at the top of
/// `pipeline::run` / `build::run_pipeline`, so every `?` in between has to pass
/// them through: one drives the cloud gate, the other the single rebuild.
#[test]
fn a_classified_stop_survives_being_carried_as_an_error() {
    fn carry(stopped: BuildStopped) -> Result<(), BuildStopped> {
        Err(stopped)?;
        Ok(())
    }
    let deferred = carry(BuildStopped::deferred("still downloading")).unwrap_err();
    assert!(deferred.is_deferred(), "`?` must pass a deferred stop through unchanged");
    assert_eq!(deferred.into_message(), "still downloading");

    let discarded = carry(BuildStopped::discarded("evicted, and removed")).unwrap_err();
    assert!(discarded.is_discarded(), "and a discarded one — the rebuild is decided from it");
    assert!(!discarded.is_deferred(), "a discard is not a wait: nothing has to arrive");
}

/// A discard already unlinked the file it stopped on, so one more attempt is
/// the whole fix.
///
/// What this can prove is that the closure runs twice. That it is a *closure*
/// and not a future is the other half: `build_inner` consumes its
/// `SlotResolver` (`FnOnce`), so the resolver must be minted per call, and
/// `build.rs`'s `attempt` closure is where that happens. A counter here would
/// only count the increments this test itself writes.
#[tokio::test]
async fn a_discarded_stop_builds_once_more() {
    use std::cell::Cell;

    let attempts = Cell::new(0usize);

    let built: Result<&str, String> = retry_once_after_discard(|| {
        let attempt = attempts.get() + 1;
        attempts.set(attempt);
        async move {
            if attempt == 1 {
                Err(BuildStopped::discarded("evicted staged page, removed"))
            } else {
                Ok("built")
            }
        }
    })
    .await;

    assert_eq!(built.unwrap(), "built");
    assert_eq!(attempts.get(), 2, "a discard must be retried exactly once");
}

/// The retry is a single shot, and only for a discard. A second consecutive
/// discard means the removal settled nothing, and a fatal or deferred stop has
/// nothing to re-run for: deferring is a wait the watcher ends, not a rebuild.
#[tokio::test]
async fn only_a_first_discard_is_retried() {
    use std::cell::Cell;

    for (stopped, expected_attempts) in [
        (BuildStopped::discarded("still evicted"), 2),
        (BuildStopped::from("invalid frontmatter in posts/hello.md"), 1),
        (BuildStopped::deferred("still downloading"), 1),
    ] {
        let attempts = Cell::new(0usize);
        let message = stopped.message().to_string();
        let stopped = &stopped;
        let out: Result<(), String> = retry_once_after_discard(|| {
            attempts.set(attempts.get() + 1);
            let stopped = stopped.clone();
            async move { Err(stopped) }
        })
        .await;

        assert_eq!(out.unwrap_err(), message, "the message survives the flattening");
        assert_eq!(attempts.get(), expected_attempts, "for {message}");
    }
}
