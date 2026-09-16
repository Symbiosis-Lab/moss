use super::*;

const HOLD_ENV: &str = "MOSS_TEST_HOLD_STACK_LOCK";
const CHILD_TEST: &str = "deploy::stack_activity::tests::child_lock_holder";

fn short() -> Duration {
    Duration::from_millis(400)
}

/// The property the process-local `Mutex` this replaced could not have: two
/// INDEPENDENT open file descriptions on the same path exclude each other.
///
/// `flock` is held on the open file description, not on the process, so this is
/// a faithful test of the cross-process behaviour and not merely of a mutex —
/// two `File::open`s in one process are as separate to the kernel as two
/// processes are. What it cannot see is release-on-death; that is the next test.
#[test]
fn two_open_handles_exclude_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let a = StackActivity::at(tmp.path());
    let b = StackActivity::at(tmp.path());

    assert!(!a.busy(), "a fresh root is not busy");

    let held = a.hold("installing the OnionPress stack", short(), || {}).unwrap();

    assert!(b.busy(), "a second handle sees the first handle's lock");
    let refused = b
        .hold("removing the OnionPress stack", short(), || {})
        .expect_err("the second handle must be refused, not served");

    // The refusal is the whole explanation a terminal author gets, so it names
    // the holder and the one action available.
    assert!(
        refused.contains("installing the OnionPress stack"),
        "the refusal names what is holding the lock: {refused}"
    );
    assert!(
        refused.contains(&format!("pid {}", std::process::id())),
        "the refusal names the holder's pid: {refused}"
    );
    assert!(refused.contains("try again"), "the refusal says what to do: {refused}");
    assert!(
        refused.starts_with("cannot start removing the OnionPress stack"),
        "the refusal names what was refused: {refused}"
    );

    drop(held);
    assert!(!b.busy(), "releasing the lock releases it for every handle");
    b.hold("removing the OnionPress stack", short(), || {}).unwrap();
}

/// `queued` runs once when the lock is contended and never when it is free.
/// The wait is minutes long on a real install, and it sat before the install
/// pipeline's first progress emission — the callback is what keeps a queued
/// caller from reading as stuck at 0%.
#[test]
fn the_wait_is_announced_once_and_only_when_contended() {
    let tmp = tempfile::tempdir().unwrap();
    let activity = StackActivity::at(tmp.path());
    let announced = std::cell::Cell::new(0usize);

    let held = activity.hold("installing the OnionPress stack", short(), || {
        announced.set(announced.get() + 1)
    });
    assert_eq!(announced.get(), 0, "a free lock is not a queue");

    let _ = StackActivity::at(tmp.path()).hold("starting the OnionPress stack", short(), || {
        announced.set(announced.get() + 1)
    });
    assert_eq!(announced.get(), 1, "a contended lock announces exactly once");
    drop(held);
}

/// The property this whole fix rests on and the one a same-process test cannot
/// reach: a holder that DIES releases the lock, without moss noticing or
/// cleaning anything up.
///
/// Without it a crashed installer would wedge every later install, start and
/// uninstall on the machine permanently, which is strictly worse than the
/// process-local mutex it replaced. So it is proven against a real child
/// process and a real `SIGKILL`.
#[test]
fn a_killed_holders_lock_becomes_acquirable() {
    let tmp = tempfile::tempdir().unwrap();
    let activity = StackActivity::at(tmp.path());

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--test-threads=1", "--nocapture"])
        .env(HOLD_ENV, tmp.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("re-exec the test binary as a second process");

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !activity.busy() {
        assert!(
            std::time::Instant::now() < deadline,
            "the child never took the lock — child test name {CHILD_TEST} may have moved"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // While the child lives, this process cannot have it: the exclusion is
    // real across processes, which is the first half of the hazard.
    assert!(
        activity.hold("installing the OnionPress stack", short(), || {}).is_err(),
        "a live foreign holder must exclude this process"
    );
    let holder = activity.holder().expect("the holder record names the child");
    assert_eq!(holder.pid, child.id(), "the record names the child's pid");

    child.kill().expect("SIGKILL the holder");
    child.wait().expect("reap the holder");

    // No cleanup step runs in between. The kernel is the only thing that
    // released it.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match activity.hold("installing the OnionPress stack", short(), || {}) {
            Ok(_) => break,
            Err(e) => assert!(
                std::time::Instant::now() < deadline,
                "a dead holder's lock never became acquirable: {e}"
            ),
        }
    }
}

/// Not a test. The child half of [`a_killed_holders_lock_becomes_acquirable`],
/// a `#[test]` only because re-executing the test binary is the one portable
/// way to get a second PROCESS out of a unit test. Inert unless the parent
/// names it and sets `HOLD_ENV`, so an ordinary suite run passes straight
/// through it.
#[test]
fn child_lock_holder() {
    let Some(root) = std::env::var_os(HOLD_ENV) else {
        return;
    };
    let _held = StackActivity::at(PathBuf::from(root))
        .hold("holding the stack lock for a test", LOCK_WAIT, || {})
        .expect("the child takes the lock");
    // Outlives the parent's assertions; the parent kills it.
    std::thread::sleep(Duration::from_secs(120));
}

/// The lease is the opposite of the lock on purpose: it outlives the process
/// that wrote it (a crashed publish's bytes may still be landing), it expires
/// on its own (a crashed publish must not disable recovery forever), and a
/// shorter one never shortens a longer one already on disk (two overlapping
/// publishes must not leave the stack unprotected).
#[test]
fn the_publish_lease_outlives_its_writer_and_never_shortens() {
    let tmp = tempfile::tempdir().unwrap();
    let activity = StackActivity::at(tmp.path());

    assert_eq!(activity.publish_lease_until(), 0, "no lease is 0, which allows recovery");

    activity.note_publish(1_000, 600);
    assert_eq!(activity.publish_lease_until(), 1_600);

    // A second, shorter publish cannot pull the protection back in.
    StackActivity::at(tmp.path()).note_publish(1_000, 60);
    assert_eq!(activity.publish_lease_until(), 1_600, "a shorter lease never wins");

    // A later one extends it.
    StackActivity::at(tmp.path()).note_publish(1_500, 600);
    assert_eq!(activity.publish_lease_until(), 2_100);

    // Garbage reads as no lease rather than as a lease that never ends.
    std::fs::write(activity.lease_path(), "not a number").unwrap();
    assert_eq!(activity.publish_lease_until(), 0);
}

#[test]
fn a_holder_record_that_cannot_be_parsed_costs_the_refusal_only_its_name() {
    assert_eq!(
        parse_holder("4123 1700000000 installing the OnionPress stack"),
        Some(Holder {
            pid: 4123,
            verb: "installing the OnionPress stack".to_string(),
            since: 1_700_000_000
        })
    );
    for junk in ["", "   ", "4123", "4123 1700000000", "4123 1700000000  ", "x y z"] {
        assert_eq!(parse_holder(junk), None, "unparsable: {junk:?}");
    }

    let tmp = tempfile::tempdir().unwrap();
    let activity = StackActivity::at(tmp.path());
    std::fs::create_dir_all(tmp.path()).unwrap();
    std::fs::write(activity.holder_path(), "garbage").unwrap();
    assert_eq!(activity.holder(), None);
    // And the lock still works — the record is advisory, the lock is not.
    let _held = activity.hold("installing the OnionPress stack", short(), || {}).unwrap();
    assert!(StackActivity::at(tmp.path()).busy());
}

/// `MOSS_STACKS_ROOT` is what makes every test above hermetic, so its
/// precedence over the home directory is pinned rather than assumed.
#[test]
fn the_stacks_root_falls_back_to_home_but_prefers_the_override() {
    // Read-only: no other test mutates the environment, so this observes
    // whichever arm the suite is running under without racing anything.
    match std::env::var_os("MOSS_STACKS_ROOT") {
        Some(over) => assert_eq!(stacks_root().unwrap(), PathBuf::from(over)),
        None => assert_eq!(
            stacks_root().unwrap(),
            dirs::home_dir().unwrap().join(".moss").join("stacks")
        ),
    }
}
