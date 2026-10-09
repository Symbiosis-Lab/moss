use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn pool() -> crate::build::cloud_prefetch::Prefetcher {
    crate::build::cloud_prefetch::Prefetcher::with_materializer(3, Arc::new(|_| Ok(())), false)
}

struct SessionGuard { path: String, session: Arc<crate::system::folder_session::FolderSession> }
impl SessionGuard {
    fn new(path: &Path) -> Self {
        let path = path.to_string_lossy().into_owned();
        let session = crate::system::folder_session::FolderSession::new(PathBuf::from(&path));
        crate::system::folder_session::registry().insert(path.clone(), session.clone());
        Self { path, session }
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.session.cancel.cancel();
        crate::system::folder_session::registry().remove(&self.path);
    }
}

fn refused(path: &Path, operation: StorageOperation) -> StorageFailure {
    StorageFailure::new(Some(path.to_path_buf()), operation, io::Error::from_raw_os_error(libc::EDEADLK))
}

#[test]
fn availability_recovers_same_complete_directory_operation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("first.md"), "one").unwrap();
    std::fs::write(root.path().join("second.md"), "two").unwrap();
    let path = root.path().to_path_buf();
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let attempt: Operation = Arc::new(move || {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let result = collect_directory_entries(&path, |_| Ok(vec![Ok(()), Err(io::Error::from_raw_os_error(libc::EDEADLK))].into_iter()));
            return Err(result.unwrap_err());
        }
        let entries = collect_directory_entries(&path, |p| std::fs::read_dir(p))?;
        let entries = entries.into_iter().map(|e| {
            let m = e.metadata().map_err(|err| StorageFailure::new(Some(e.path()), StorageOperation::EntryMetadata, err))?;
            Ok((e, m))
        }).collect::<Result<Vec<_>, StorageFailure>>()?;
        Ok(StorageValue::Directory { entries, hidden: 0 })
    });
    let waited = std::cell::Cell::new(false);
    let result = await_operation_in(&pool(), root.path(), OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| false, &|| waited.set(true), attempt).unwrap();
    let StorageValue::Directory { entries, .. } = &*result else { panic!("wrong operation result") };
    assert_eq!(entries.len(), 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(waited.get());
}

#[test]
fn availability_local_deadlock_is_immediate_fatal() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let result = await_operation_in(&pool(), root.path(), OperationPolicy::SourceWalk, false, Duration::from_secs(1), &|| false, &|| panic!("local errors never wait"), Arc::new(move || Err(refused(&path, StorageOperation::ReadDirOpen))));
    assert_eq!(result.unwrap_err().pending, None);
}

#[test]
fn availability_parent_eof_does_not_clear_child_metadata_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("child.md");
    let result = await_operation_in(&pool(), root.path(), OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, Arc::new(move || Err(refused(&path, StorageOperation::EntryMetadata))));
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
}

#[test]
fn availability_cancel_detaches_without_accepting_a_result() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let result = await_operation_in(&pool(), root.path(), OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| true, &|| {}, Arc::new(move || Err(refused(&path, StorageOperation::WalkEntry))));
    assert_eq!(result.unwrap_err().pending, Some(Settled::Cancelled));
}

fn wait_until(predicate: impl Fn() -> bool) {
    let start = std::time::Instant::now();
    while !predicate() {
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn availability_fixed_capacity_is_ten_native_calls_and_deduplicates_wedges() {
    use std::sync::Condvar;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let byte_gate = gate.clone();
    let pool = crate::build::cloud_prefetch::Prefetcher::with_materializer(8, Arc::new(move |_| {
        let (lock, wake) = &*byte_gate;
        let mut released = lock.lock().unwrap();
        while !*released { released = wake.wait(released).unwrap(); }
        Ok(())
    }), false);
    for i in 0..8 { pool.read_foreground(Path::new(&format!("/byte/{i}"))); }
    wait_until(|| pool.snapshot().in_flight == 8);
    let calls = Arc::new(AtomicUsize::new(0));
    let native_gate = gate.clone();
    let native_calls = calls.clone();
    let operation: Operation = Arc::new(move || {
        native_calls.fetch_add(1, Ordering::SeqCst);
        let (lock, wake) = &*native_gate;
        let mut released = lock.lock().unwrap();
        while !*released { released = wake.wait(released).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    let key = |index| OperationKey { path: PathBuf::from(format!("/structure/{index}")), policy: OperationPolicy::SourceWalk };
    let task = pool.structural_operation(key(0), operation.clone()).unwrap();
    pool.structural_operation(key(1), operation.clone()).unwrap();
    wait_until(|| calls.load(Ordering::SeqCst) == 2);
    for _ in 0..20 {
        let same = pool.structural_operation(key(0), operation.clone()).unwrap();
        assert!(Arc::ptr_eq(&same, &task));
    }
    for index in 2..20 { pool.structural_operation(key(index), operation.clone()); }
    assert_eq!(pool.snapshot().in_flight, 10);
    assert_eq!(pool.snapshot().waiting, crate::build::cloud_prefetch::BACKGROUND_WAITING);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().in_flight == 0 && pool.snapshot().waiting == 0);
}

#[test]
fn availability_detached_waiters_share_native_call_and_emit_one_recovery_arrival() {
    use std::sync::Condvar;
    let root = tempfile::tempdir().unwrap();
    let _session = SessionGuard::new(root.path());
    let path = root.path().to_path_buf();
    let pool = Arc::new(pool());
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let native_gate = gate.clone();
    let native_calls = calls.clone();
    let operation: Operation = Arc::new(move || {
        if !std::thread::current().name().is_some_and(|n| n.starts_with("moss-cloud-structure-")) {
            return Err(refused(&path, StorageOperation::ReadDirOpen));
        }
        native_calls.fetch_add(1, Ordering::SeqCst);
        let (lock, wake) = &*native_gate;
        let mut released = lock.lock().unwrap();
        while !*released { released = wake.wait(released).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_pool = pool.clone();
    let worker_cancel = cancelled.clone();
    let worker_path = root.path().to_path_buf();
    let worker_operation = operation.clone();
    let waiter = std::thread::spawn(move || await_operation_in(&worker_pool, &worker_path, OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| worker_cancel.load(Ordering::SeqCst), &|| {}, worker_operation));
    wait_until(|| calls.load(Ordering::SeqCst) == 1);
    cancelled.store(true, Ordering::SeqCst);
    assert_eq!(waiter.join().unwrap().unwrap_err().pending, Some(Settled::Cancelled));
    let next = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, operation);
    assert_eq!(next.unwrap_err().pending, Some(Settled::TimedOut));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(pool.snapshot().in_flight, 1);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().in_flight == 0);
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 1);
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn availability_worker_opts_in_without_changing_caller_or_process_policy() {
    std::thread::spawn(|| {
        unsafe extern "C" { fn getiopolicy_np(kind: i32, scope: i32) -> i32; }
        let policy = |scope| unsafe { getiopolicy_np(3, scope) };
        let process = policy(0);
        assert!(crate::platform::fail_fast_on_this_thread());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_path_buf();
        let pool = crate::build::cloud_prefetch::Prefetcher::with_materializer(1, Arc::new(|_| Ok(())), true);
        let operation: Operation = Arc::new(move || {
            if unsafe { getiopolicy_np(3, 1) } != 2 { return Err(refused(&path, StorageOperation::RootMetadata)); }
            Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
        });
        await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| false, &|| {}, operation).unwrap();
        assert_eq!(policy(1), 1);
        assert_eq!(policy(0), process);
    }).join().unwrap();
}

#[test]
fn availability_new_request_discards_completion_started_before_source_change() {
    use std::sync::{Condvar, atomic::AtomicBool};
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child.md");
    std::fs::write(&child, "old").unwrap();
    let path = root.path().to_path_buf();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let begun = Arc::new(AtomicBool::new(false));
    let worker_gate = gate.clone();
    let worker_begun = begun.clone();
    let operation: Operation = Arc::new(move || {
        if !std::thread::current().name().is_some_and(|n| n.starts_with("moss-cloud-structure-")) {
            return Err(refused(&path, StorageOperation::ReadDirOpen));
        }
        let entry = std::fs::read_dir(&path).unwrap().next().unwrap().unwrap();
        let metadata = entry.metadata().unwrap();
        worker_begun.store(true, Ordering::SeqCst);
        let (lock, wake) = &*worker_gate;
        let mut released = lock.lock().unwrap();
        while !*released { released = wake.wait(released).unwrap(); }
        Ok(StorageValue::Directory { entries: vec![(entry, metadata)], hidden: 0 })
    });
    let pool = Arc::new(pool());
    let cancel = Arc::new(AtomicBool::new(false));
    let first_pool = pool.clone();
    let first_path = root.path().to_path_buf();
    let first_op = operation.clone();
    let first_cancel = cancel.clone();
    let first = std::thread::spawn(move || await_operation_in(&first_pool, &first_path, OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| first_cancel.load(Ordering::SeqCst), &|| {}, first_op));
    wait_until(|| begun.load(Ordering::SeqCst));
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(first.join().unwrap().unwrap_err().pending, Some(Settled::Cancelled));
    std::fs::write(&child, "new content").unwrap();
    let joining = Arc::new(AtomicBool::new(false));
    let next_joining = joining.clone();
    let next_pool = pool.clone();
    let next_path = root.path().to_path_buf();
    let next = std::thread::spawn(move || await_operation_in(&next_pool, &next_path, OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| false, &|| next_joining.store(true, Ordering::SeqCst), operation));
    wait_until(|| joining.load(Ordering::SeqCst));
    std::thread::sleep(Duration::from_millis(20));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let result = next.join().unwrap().unwrap();
    let StorageValue::Directory { entries, .. } = &*result else { panic!("wrong operation") };
    assert_eq!(entries[0].1.len(), 11, "a newer request cannot consume the older walk's receipt");
}

#[test]
fn availability_capacity_pending_receives_one_wake_when_other_work_frees_admission() {
    use std::sync::Condvar;
    let pool = pool();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let native_gate = gate.clone();
    let held: Operation = Arc::new(move || {
        let (lock, wake) = &*native_gate;
        let mut released = lock.lock().unwrap();
        while !*released { released = wake.wait(released).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    let key = |i| OperationKey { path: PathBuf::from(format!("/unrelated/{i}")), policy: OperationPolicy::SourceWalk };
    for i in 0..2 { pool.structural_operation(key(i), held.clone()).unwrap(); }
    wait_until(|| pool.snapshot().in_flight == 2);
    for i in 2..14 { pool.structural_operation(key(i), held.clone()).unwrap(); }
    assert_eq!(pool.snapshot().waiting, crate::build::cloud_prefetch::BACKGROUND_WAITING);
    let root = tempfile::tempdir().unwrap();
    let session = crate::system::folder_session::FolderSession::new(root.path().to_path_buf());
    let registered = root.path().to_str().unwrap().to_string();
    crate::system::folder_session::registry().insert(registered.clone(), session.clone());
    std::fs::write(root.path().join("story.md"), "# story").unwrap();
    let path = root.path().to_path_buf();
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = attempts.clone();
    let source: Operation = Arc::new(move || {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 { return Err(refused(&path, StorageOperation::ReadDirOpen)); }
        let entries = collect_directory_entries(&path, |p| std::fs::read_dir(p))?.into_iter().map(|e| {
            let metadata = e.metadata().unwrap();
            (e, metadata)
        }).collect();
        Ok(StorageValue::Directory { entries, hidden: 0 })
    });
    let result = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, source.clone());
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 0);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().waiting == 0 && pool.snapshot().in_flight == 0);
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 1, "admission refusal must retain a recovery wake");
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 0);
    let result = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| false, &|| {}, source).unwrap();
    let StorageValue::Directory { entries, .. } = &*result else { panic!("wrong operation") };
    assert_eq!(entries.len(), 1);
    crate::system::folder_session::registry().remove(&registered);
    session.cancel.cancel();
}

#[test]
fn availability_returned_refusals_yield_workers_to_a_healthy_third_operation() {
    use std::sync::atomic::AtomicBool;
    let pool = pool();
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    for i in 0..2 {
        let attempts = calls.clone();
        let released = release.clone();
        let path = PathBuf::from(format!("/stuck/{i}"));
        let operation: Operation = Arc::new(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            if !released.load(Ordering::SeqCst) { return Err(refused(&path, StorageOperation::ReadDirOpen)); }
            Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
        });
        pool.structural_operation(OperationKey { path: PathBuf::from(format!("/stuck/{i}")), policy: OperationPolicy::SourceWalk }, operation).unwrap();
    }
    wait_until(|| calls.load(Ordering::SeqCst) >= 2);
    let healthy = Arc::new(AtomicBool::new(false));
    let completed = healthy.clone();
    pool.structural_operation(OperationKey { path: PathBuf::from("/healthy"), policy: OperationPolicy::SourceWalk }, Arc::new(move || {
        completed.store(true, Ordering::SeqCst);
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    })).unwrap();
    let start = std::time::Instant::now();
    while !healthy.load(Ordering::SeqCst) && start.elapsed() < Duration::from_millis(250) { std::thread::sleep(Duration::from_millis(1)); }
    let progressed_before_release = healthy.load(Ordering::SeqCst);
    release.store(true, Ordering::SeqCst);
    wait_until(|| pool.snapshot().in_flight == 0 && pool.snapshot().waiting == 0);
    assert!(progressed_before_release, "returned provider errors must yield both reserved workers");
}

#[test]
fn availability_deferred_operation_hard_failure_wakes_its_owner() {
    let pool = pool();
    let (send, receive) = std::sync::mpsc::channel();
    pool.set_settlement_emitter(Arc::new(move |event| { send.send(event).unwrap(); }));
    let root = tempfile::tempdir().unwrap();
    let _session = SessionGuard::new(root.path());
    let path = root.path().to_path_buf();
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = attempts.clone();
    let operation: Operation = Arc::new(move || {
        let count = calls.fetch_add(1, Ordering::SeqCst);
        if count < 2 { return Err(refused(&path, StorageOperation::EntryMetadata)); }
        Err(StorageFailure::new(Some(path.clone()), StorageOperation::EntryMetadata, io::Error::from_raw_os_error(libc::EACCES)))
    });
    let result = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, operation.clone());
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    wait_until(|| pool.snapshot().in_flight == 0 && pool.snapshot().waiting == 0);
    let arrivals = pool.take_structural_arrivals(root.path());
    assert_eq!(arrivals.wake_count(), 1, "a terminal hard error must settle prior Pending");
    let event = receive.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(event, crate::types::events::MossEvent::SourceStructureSettled {
        folder_path, path, outcome: crate::types::events::SourceStructureOutcome::Failed,
    } if folder_path == root.path().to_string_lossy() && path == root.path().to_string_lossy()));
    let resumed = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_secs(1), &|| false, &|| panic!("hard errors never wait"), operation).unwrap_err();
    assert_eq!(resumed.pending, None);
    assert_eq!(resumed.failure.raw_os_error(), Some(libc::EACCES));
    assert_eq!(attempts.load(Ordering::SeqCst), 4, "terminal failure cannot keep retrying");
}

#[test]
fn availability_closed_folder_drops_deferred_retry_and_emits_no_old_completion() {
    let pool = pool();
    let (send, receive) = std::sync::mpsc::channel();
    pool.set_settlement_emitter(Arc::new(move |event| { send.send(event).unwrap(); }));
    let root = tempfile::tempdir().unwrap();
    let registered = root.path().to_str().unwrap().to_owned();
    let session = crate::system::folder_session::FolderSession::new(root.path().to_path_buf());
    crate::system::folder_session::registry().insert(registered.clone(), session.clone());
    let path = root.path().to_path_buf();
    let operation: Operation = Arc::new(move || Err(refused(&path, StorageOperation::ReadDirOpen)));
    let result = await_operation_in(&pool, root.path(), OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, operation);
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    session.cancel.cancel();
    let start = Instant::now();
    while pool.snapshot().waiting > 0 || pool.snapshot().in_flight > 0 {
        if start.elapsed() >= Duration::from_secs(1) { break; }
        std::thread::sleep(Duration::from_millis(1));
    }
    crate::system::folder_session::registry().remove(&registered);
    assert_eq!(pool.snapshot().waiting + pool.snapshot().in_flight, 0, "closed session must release returned-refusal tasks");
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 0);
    assert!(receive.try_recv().is_err(), "closed folder cannot receive an obsolete terminal event");
}

#[test]
fn availability_standalone_editor_settles_on_typed_bus_without_a_sweep_or_build() {
    let pool = Arc::new(pool());
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let (send, receive) = std::sync::mpsc::channel();
    let reentrant = Arc::downgrade(&pool);
    pool.set_settlement_emitter(Arc::new(move |event| {
        let _snapshot = reentrant.upgrade().unwrap().snapshot();
        send.send(event).unwrap();
    }));
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = attempts.clone();
    let operation: Operation = Arc::new(move || {
        if calls.fetch_add(1, Ordering::SeqCst) < 2 { return Err(refused(&path, StorageOperation::ReadDirNext)); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    let result = await_operation_in(&pool, root.path(), OperationPolicy::EditorDirectory { project: root.path().to_path_buf(), show_internal: false }, true, Duration::from_millis(20), &|| false, &|| {}, operation);
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    wait_until(|| pool.snapshot().waiting + pool.snapshot().in_flight == 0);
    let event = receive.recv_timeout(Duration::from_secs(1)).expect("standalone tree recovery cannot depend on a sweep or successful build");
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(json, serde_json::json!({ "kind": "SourceStructureSettled", "payload": { "folder_path": root.path().to_string_lossy(), "path": root.path().to_string_lossy(), "outcome": "Ready" }}));
    assert!(receive.try_recv().is_err(), "one terminal read emits once");
}

#[test]
fn availability_full_queue_replays_exact_standalone_editor_policy_and_emits_after_eof() {
    let pool = pool();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let held_gate = gate.clone();
    let held: Operation = Arc::new(move || {
        let mut release = held_gate.0.lock().unwrap();
        while !*release { release = held_gate.1.wait(release).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    for i in 0..2 { pool.structural_operation(OperationKey { path: format!("/occupied/{i}").into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap(); }
    wait_until(|| pool.snapshot().in_flight == 2);
    for i in 2..14 { pool.structural_operation(OperationKey { path: format!("/occupied/{i}").into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap(); }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("visible.md"), "# visible").unwrap();
    std::fs::create_dir(root.path().join(".moss")).unwrap();
    let _fault = TestFault::install(root.path(), root.path(), StorageOperation::ReadDirOpen, 1, Duration::from_millis(20));
    let (send, receive) = std::sync::mpsc::channel();
    pool.set_settlement_emitter(Arc::new(move |event| { send.send(event).unwrap(); }));
    let policy = OperationPolicy::EditorDirectory { project: root.path().into(), show_internal: true };
    let result = await_operation_in(&pool, root.path(), policy.clone(), true, Duration::from_millis(20), &|| false, &|| {}, directory_operation(root.path(), root.path(), true));
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    assert!(receive.try_recv().is_err(), "capacity refusal cannot emit settled storage");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let event = receive.recv_timeout(Duration::from_secs(2)).expect("the owner must re-admit a standalone editor without a sweep");
    assert!(matches!(event, crate::types::events::MossEvent::SourceStructureSettled { outcome: crate::types::events::SourceStructureOutcome::Ready, .. }));
    wait_until(|| pool.snapshot().waiting + pool.snapshot().in_flight == 0);
    let snapshot = await_operation_in(&pool, root.path(), policy, true, Duration::from_secs(1), &|| false, &|| {}, directory_operation(root.path(), root.path(), true)).unwrap();
    let StorageValue::Directory { entries, .. } = &*snapshot else { panic!("wrong policy") };
    assert_eq!(entries.len(), 2, "replayed show_internal policy must include .moss");
}

#[test]
fn availability_editor_admission_intents_are_bounded_and_closed_owners_are_pruned() {
    let pool = pool();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let held_gate = gate.clone();
    let held: Operation = Arc::new(move || {
        let mut release = held_gate.0.lock().unwrap();
        while !*release { release = held_gate.1.wait(release).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    for i in 0..2 { pool.structural_operation(OperationKey { path: format!("/bound/{i}").into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap(); }
    wait_until(|| pool.snapshot().in_flight == 2);
    for i in 2..14 { pool.structural_operation(OperationKey { path: format!("/bound/{i}").into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap(); }
    let root = tempfile::tempdir().unwrap();
    let registered = root.path().join("0").to_string_lossy().into_owned();
    let session = crate::system::folder_session::FolderSession::new(registered.clone().into());
    crate::system::folder_session::registry().insert(registered.clone(), session.clone());
    let mut faults = Vec::new();
    for i in 0..=12 {
        let path = root.path().join(i.to_string());
        std::fs::create_dir(&path).unwrap();
        faults.push(TestFault::install(&path, &path, StorageOperation::ReadDirOpen, 1, Duration::from_millis(20)));
        let policy = OperationPolicy::EditorDirectory { project: path.clone(), show_internal: false };
        let result = await_operation_in(&pool, &path, policy, true, Duration::from_millis(20), &|| false, &|| {}, directory_operation(&path, &path, false)).unwrap_err();
        assert_eq!(result.pending, (i < 12).then_some(Settled::TimedOut), "a request beyond the bounded intent backlog must report admission exhaustion");
    }
    session.cancel.cancel();
    let path = root.path().join("replacement");
    std::fs::create_dir(&path).unwrap();
    faults.push(TestFault::install(&path, &path, StorageOperation::ReadDirOpen, 1, Duration::from_millis(20)));
    let policy = OperationPolicy::EditorDirectory { project: path.clone(), show_internal: false };
    let result = await_operation_in(&pool, &path, policy, true, Duration::from_millis(20), &|| false, &|| {}, directory_operation(&path, &path, false)).unwrap_err();
    assert_eq!(result.pending, Some(Settled::TimedOut), "a closed owner's lightweight intent cannot consume future admission");
    let old = OperationKey { path: registered.clone().into(), policy: OperationPolicy::EditorDirectory { project: registered.clone().into(), show_internal: false } };
    assert!(!pool.structural_recovery_retained(&old));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().waiting + pool.snapshot().in_flight == 0);
    crate::system::folder_session::registry().remove(&registered);
}

#[test]
fn availability_wedged_optional_publications_preserve_critical_source_and_foreground_capacity() {
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let pool = crate::build::cloud_prefetch::Prefetcher::with_materializer(8, Arc::new(move |_| { observed.fetch_add(1, Ordering::SeqCst); Ok(()) }), false);
    let native = Arc::new(AtomicUsize::new(0));
    let active = native.clone();
    let held_gate = gate.clone();
    let held: Operation = Arc::new(move || {
        active.fetch_add(1, Ordering::SeqCst);
        let mut release = held_gate.0.lock().unwrap();
        while !*release { release = held_gate.1.wait(release).unwrap(); }
        Ok(StorageValue::Published(0))
    });
    for i in 0..6 { pool.cache_publication(OperationKey { path: format!("/replica/{i}").into(), policy: OperationPolicy::CachePublication { root: "/replica".into() } }, held.clone()).unwrap(); }
    wait_until(|| native.load(Ordering::SeqCst) == 6);
    for i in 6..30 { pool.cache_publication(OperationKey { path: format!("/replica/{i}").into(), policy: OperationPolicy::CachePublication { root: "/replica".into() } }, held.clone()); }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("complete.md"), "# complete").unwrap();
    let task = pool.structural_operation(OperationKey { path: root.path().into(), policy: OperationPolicy::SourceWalk }, directory_operation(root.path(), root.path(), false)).unwrap();
    pool.read_foreground(Path::new("/foreground/one"));
    pool.read_foreground(Path::new("/foreground/two"));
    let started = Instant::now();
    while (task.state.lock().unwrap().result.is_none() || reads.load(Ordering::SeqCst) < 2) && started.elapsed() < Duration::from_millis(250) { std::thread::sleep(Duration::from_millis(1)); }
    let progressed = task.state.lock().unwrap().result.is_some();
    let foreground_progressed = reads.load(Ordering::SeqCst) == 2;
    assert_eq!(native.load(Ordering::SeqCst), 6, "optional jobs cannot consume the foreground reserve");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().waiting + pool.snapshot().in_flight == 0);
    assert!(progressed, "wedged optional writes cannot starve complete source/tree reads");
    assert!(foreground_progressed, "foreground byte inputs must still progress");
}

#[test]
fn availability_new_candidate_after_old_completion_cannot_wait_without_admission() {
    let pool = Arc::new(pool());
    let old_gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let blocking = |gate: Arc<(Mutex<bool>, std::sync::Condvar)>| -> Operation { Arc::new(move || {
        let mut release = gate.0.lock().unwrap();
        while !*release { release = gate.1.wait(release).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    }) };
    let old_key = OperationKey { path: "/old-native".into(), policy: OperationPolicy::SourceWalk };
    let operation = blocking(old_gate.clone());
    let old = pool.structural_operation(old_key.clone(), operation.clone()).unwrap();
    old.state.lock().unwrap().deferred = true;
    let held = blocking(gate.clone());
    for i in 1..14 { pool.structural_operation(OperationKey { path: format!("/full/{i}").into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap(); }
    wait_until(|| pool.snapshot().in_flight == 2);
    let weak = Arc::downgrade(&pool);
    pool.set_settlement_emitter(Arc::new(move |_| {
        weak.upgrade().unwrap().structural_operation(OperationKey { path: "/replacement-native".into(), policy: OperationPolicy::SourceWalk }, held.clone()).unwrap();
    }));
    let subscriber = pool.clone();
    let waiter = std::thread::spawn(move || wait_for_operation(&subscriber, &old_key.path, old_key.policy, Duration::from_secs(1), &|| false, &|| {}, operation, Arc::new(refused(Path::new("/old-native"), StorageOperation::WalkEntry))));
    wait_until(|| old.state.lock().unwrap().waiters == 2);
    *old_gate.0.lock().unwrap() = true;
    old_gate.1.notify_all();
    let result = waiter.join().unwrap().unwrap_err();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| pool.snapshot().in_flight + pool.snapshot().waiting == 0);
    assert_eq!(result.pending, None, "a fresh request with no admitted operation or retained recovery intent cannot remain Pending");
    assert!(result.failure.to_string().contains("admission capacity exhausted"));
}

#[test]
fn availability_new_record_revision_during_native_publish_requires_another_complete_transaction() {
    let pool = pool();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("record.json");
    let revision = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let latest = revision.clone();
    let native_gate = gate.clone();
    let output = path.clone();
    let operation: Operation = Arc::new(move || {
        let version = latest.load(Ordering::Acquire);
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut release = native_gate.0.lock().unwrap();
            while !*release { release = native_gate.1.wait(release).unwrap(); }
        }
        std::fs::write(&output, version.to_string()).unwrap();
        Ok(StorageValue::Published(version))
    });
    let key = OperationKey { path: path.clone(), policy: OperationPolicy::CachePublication { root: root.path().into() } };
    let task = pool.cache_publication_revision(key.clone(), operation.clone(), Some(revision.clone())).unwrap();
    wait_until(|| calls.load(Ordering::SeqCst) == 1);
    revision.store(2, Ordering::Release);
    assert!(Arc::ptr_eq(&task, &pool.cache_publication_revision(key, operation, Some(revision)).unwrap()));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| task.state.lock().unwrap().result.is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 2, "a completed older revision cannot settle a newer owned change");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "2");
}

#[test]
fn availability_standalone_editor_signals_do_not_accumulate_unconsumed_sweep_receipts() {
    let pool = pool();
    let root = tempfile::tempdir().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    pool.set_settlement_emitter(Arc::new(move |event| { send.send(event).unwrap(); }));
    for i in 0..24 {
        let path = root.path().join(i.to_string());
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("visible.md"), "# complete").unwrap();
        let native = directory_operation(&path, &path, false);
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let native_gate = gate.clone();
        let target = path.clone();
        let operation: Operation = Arc::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 { return Err(refused(&target, StorageOperation::ReadDirOpen)); }
            let mut release = native_gate.0.lock().unwrap();
            while !*release { release = native_gate.1.wait(release).unwrap(); }
            native()
        });
        let result = await_operation_in(&pool, &path, OperationPolicy::EditorDirectory { project: path.clone(), show_internal: false }, true, Duration::from_millis(5), &|| false, &|| {}, operation);
        assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let event = receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(event, crate::types::events::MossEvent::SourceStructureSettled { path: settled, outcome: crate::types::events::SourceStructureOutcome::Ready, .. } if settled == path.to_string_lossy()));
    }
    assert_eq!(pool.structural_receipts_for_test(), 0, "standalone document trees have no folder sweep to consume receipts");
}

#[test]
fn availability_reopened_same_folder_retains_a_fresh_request_after_old_native_call_returns() {
    for registered in [true, false] {
    let pool = pool();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let folder = path.to_string_lossy().into_owned();
    let child = path.join("visible.md");
    std::fs::write(&child, "old").unwrap();
    let old = crate::system::folder_session::FolderSession::new(path.clone());
    crate::system::folder_session::registry().insert(folder.clone(), old.clone());
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let collected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let old_collected = collected.clone();
    let native = directory_operation(&path, &path, false);
    let native_gate = gate.clone();
    let operation: Operation = Arc::new(move || {
        let snapshot = native()?;
        old_collected.store(true, Ordering::Release);
        let mut released = native_gate.0.lock().unwrap();
        while !*released { released = native_gate.1.wait(released).unwrap(); }
        Ok(snapshot)
    });
    pool.structural_operation(OperationKey { path: path.clone(), policy: OperationPolicy::SourceWalk }, operation).unwrap();
    wait_until(|| collected.load(Ordering::Acquire));
    old.cancel.cancel();
    crate::system::folder_session::registry().remove(&folder);
    let fresh = registered.then(|| crate::system::folder_session::FolderSession::new(path.clone()));
    if let Some(fresh) = &fresh { crate::system::folder_session::registry().insert(folder.clone(), fresh.clone()); }
    std::fs::write(&child, "new complete candidate").unwrap();
    let native = directory_operation(&path, &path, false);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let target = path.clone();
    let operation: Operation = Arc::new(move || {
        if !std::thread::current().name().is_some_and(|name| name.starts_with("moss-cloud-structure-")) { return Err(refused(&target, StorageOperation::ReadDirOpen)); }
        count.fetch_add(1, Ordering::SeqCst);
        let snapshot = native()?;
        let StorageValue::Directory { entries, .. } = &snapshot else { panic!("wrong candidate") };
        assert_eq!(entries[0].1.len(), "new complete candidate".len() as u64, "a new session needs a newly collected complete candidate");
        Ok(snapshot)
    });
    let (send, receive) = std::sync::mpsc::channel();
    pool.set_settlement_emitter(Arc::new(move |event| { send.send(event).unwrap(); }));
    let result = await_operation_in(&pool, &path, OperationPolicy::SourceWalk, true, Duration::from_millis(20), &|| false, &|| {}, operation);
    assert_eq!(result.unwrap_err().pending, Some(Settled::TimedOut));
    assert_eq!(pool.snapshot().in_flight, 1, "a reopened session cannot replace a blocked native worker");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let event = receive.recv_timeout(Duration::from_secs(2));
    wait_until(|| pool.snapshot().in_flight + pool.snapshot().waiting == 0);
    if let Some(fresh) = &fresh { fresh.cancel.cancel(); }
    crate::system::folder_session::registry().remove(&folder);
    assert!(event.is_ok(), "the timed-out new session must retain a fresh recovery request, independently of the obsolete owner's cancellation");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(event.unwrap(), crate::types::events::MossEvent::SourceStructureSettled { outcome: crate::types::events::SourceStructureOutcome::Ready, .. }));
    }
}

#[test]
fn availability_new_record_delete_intent_survives_an_older_native_read_error() {
    let pool = pool();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("record.json");
    std::fs::write(&path, "old invalid record").unwrap();
    let revision = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let current = revision.clone();
    let native_gate = gate.clone();
    let target = path.clone();
    let operation: Operation = Arc::new(move || {
        let version = current.load(Ordering::Acquire);
        count.fetch_add(1, Ordering::SeqCst);
        if version == 1 {
            let mut released = native_gate.0.lock().unwrap();
            while !*released { released = native_gate.1.wait(released).unwrap(); }
            return Err(StorageFailure::new(Some(target.clone()), StorageOperation::CacheRead, io::Error::from_raw_os_error(libc::EACCES)));
        }
        std::fs::remove_file(&target).unwrap();
        Ok(StorageValue::Published(version))
    });
    let key = OperationKey { path: path.clone(), policy: OperationPolicy::CachePublication { root: root.path().into() } };
    let task = pool.cache_publication_revision(key.clone(), operation.clone(), Some(revision.clone())).unwrap();
    wait_until(|| calls.load(Ordering::SeqCst) == 1);
    revision.store(2, Ordering::Release);
    assert!(Arc::ptr_eq(&task, &pool.cache_publication_revision(key, operation, Some(revision)).unwrap()));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| task.state.lock().unwrap().result.is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 2, "an older read failure cannot discard a newer owned delete");
    assert!(!path.exists());
}

#[test]
fn availability_partial_sweep_cannot_emit_a_structural_ready_receipt() {
    let root = tempfile::tempdir().unwrap();
    let _session = SessionGuard::new(root.path());
    let pool = pool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let emitted = events.clone();
    pool.set_settlement_emitter(Arc::new(move |event| emitted.lock().unwrap().push(event)));
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let native_gate = gate.clone();
    let op: Operation = Arc::new(move || {
        let mut released = native_gate.0.lock().unwrap();
        while !*released { released = native_gate.1.wait(released).unwrap(); }
        Ok(StorageValue::Sweep(SweepSnapshot { deadline_blown: true, resume: Some("cursor.md".into()), ..Default::default() }))
    });
    let key = OperationKey { path: root.path().into(), policy: OperationPolicy::SweepWalk { budget: Some(Duration::from_millis(40)), resume: None } };
    let task = pool.structural_operation(key, op).unwrap();
    { let mut state = task.state.lock().unwrap(); state.waiters = 0; state.deferred = true; }
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_until(|| task.state.lock().unwrap().result.is_some());
    assert!(events.lock().unwrap().is_empty(), "bounded measurement is not complete structure");
    assert_eq!(pool.take_structural_arrivals(root.path()).wake_count(), 0);
}
