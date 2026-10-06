use super::*;

use std::time::Duration;

/// No cadence attached (the CLI, a one-shot build): derived work runs eagerly.
#[tokio::test]
async fn a_session_without_a_cadence_has_an_open_gate() {
    let session = FolderSession::new(std::path::PathBuf::from("/tmp/no-cadence"));
    let gate = session.derived_work_gate();
    assert!(!gate.is_closed());
    tokio::time::timeout(Duration::from_secs(1), gate.opened())
        .await
        .expect("an open gate must not wait");
    assert!(!DerivedWorkGate::of(None).is_closed());
}

/// The gate follows the attached cadence, and a gate taken while hidden opens
/// on the flip to `Live`.
#[tokio::test]
async fn the_gate_closes_while_hidden_and_opens_on_live() {
    let session = FolderSession::new(std::path::PathBuf::from("/tmp/cadence"));
    let (tx, rx) = watch::channel(Cadence::Live);
    session.follow_cadence(rx);
    assert!(!session.derived_work_gate().is_closed());

    tx.send_replace(Cadence::Background);
    let gate = session.derived_work_gate();
    assert!(gate.is_closed());
    let waiting = tokio::spawn(async move { gate.opened().await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "a closed gate must wait");

    tx.send_replace(Cadence::Live);
    tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .expect("the flip to Live must open the gate")
        .unwrap();
}

/// A cadence that can never flip again must not strand derived work.
#[tokio::test]
async fn a_gate_whose_cadence_sender_is_gone_is_open() {
    let (tx, rx) = watch::channel(Cadence::Background);
    let gate = DerivedWorkGate::following(rx);
    assert!(gate.is_closed());
    drop(tx);
    assert!(!gate.is_closed(), "a cadence that can never flip again must not hold work");
    tokio::time::timeout(Duration::from_secs(1), gate.opened())
        .await
        .expect("a closed channel must open the gate");
}

/// The host hands its cadence to `ops::watch::start`; the folder's session is
/// where every derived-work lane reads it from.
#[tokio::test]
async fn a_watch_start_attaches_its_cadence_to_the_folder_session() {
    use crate::ops::watch::{start, worker, RebuildAttempt, RebuildDispatch, WatchConfig};

    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap().to_string_lossy().to_string();
    // Inserted directly: `register_session` drains every other session in the
    // process, including concurrently running tests' own.
    let session = FolderSession::new(std::path::PathBuf::from(&folder));
    super::super::registry().insert(folder.clone(), session.clone());
    let (cadence_tx, cadence) = watch::channel(Cadence::Live);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let dispatch: RebuildDispatch = Arc::new(|_| Box::pin(async {}));
    let attempt: RebuildAttempt =
        Arc::new(|_, _| Box::pin(async { worker::AttemptOutcome::Completed }));
    start(WatchConfig {
        folder_path: folder.clone(),
        spawner: Arc::new(crate::build::ports::spawner::TokioSpawner),
        shutdown_rx,
        emit: Arc::new(|_| {}),
        dispatch,
        attempt,
        cadence,
    })
    .await;

    cadence_tx.send_replace(Cadence::Background);
    assert!(session.derived_work_gate().is_closed(), "the session must follow the watch's cadence");

    let _ = shutdown_tx.send(());
    super::super::registry().remove(&folder);
}

/// Work already under way while hidden runs at background QoS, and returns to
/// the thread's own class when the window is visible again.
#[cfg(target_os = "macos")]
#[test]
fn pacing_lowers_a_thread_to_background_qos_while_hidden_and_restores_it() {
    use libc::qos_class_t::{QOS_CLASS_BACKGROUND, QOS_CLASS_USER_INITIATED};
    let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tx, rx) = watch::channel(Cadence::Background);
    let gate = DerivedWorkGate::following(rx);
    std::thread::spawn(move || {
        // SAFETY: the calling thread's own QoS.
        assert_eq!(unsafe { libc::pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0) }, 0);

        gate.pace_this_thread();
        assert_eq!(super::qos::current().0 as u32, QOS_CLASS_BACKGROUND as u32);
        gate.pace_this_thread();
        assert_eq!(super::qos::current().0 as u32, QOS_CLASS_BACKGROUND as u32);

        tx.send_replace(Cadence::Live);
        gate.pace_this_thread();
        assert_eq!(super::qos::current().0 as u32, QOS_CLASS_USER_INITIATED as u32);
    })
    .join()
    .unwrap();
}

/// A publish waits on the encodes in flight, so while one is under way they
/// run at the thread's own class even though the window is hidden.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn pacing_leaves_a_thread_alone_while_a_publish_is_in_flight() {
    use libc::qos_class_t::QOS_CLASS_USER_INITIATED;
    let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_tx, rx) = watch::channel(Cadence::Background);
    let gate = DerivedWorkGate::following(rx);
    let sink = crate::deploy::progress::silent();
    let class = crate::deploy::freeze::with_publish_guard(&sink, async move {
        Ok(std::thread::spawn(move || {
            // SAFETY: the calling thread's own QoS.
            assert_eq!(unsafe { libc::pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0) }, 0);
            gate.pace_this_thread();
            super::qos::current().0 as u32
        })
        .join()
        .unwrap())
    })
    .await
    .unwrap();
    assert_eq!(class, QOS_CLASS_USER_INITIATED as u32);
}
