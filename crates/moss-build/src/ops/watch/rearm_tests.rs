//! Re-arming a watched folder: `start` called again while the first watcher
//! task is still alive, then the first task shut down.

use std::sync::{Arc, Mutex};

use crate::build::ports::spawner::{Joining, Spawner, Task};
use crate::ops::watch::cadence::Cadence;
use crate::ops::watch::{start, worker, RebuildAttempt, RebuildDispatch, WatchConfig};

/// Runs tasks on tokio and keeps their handles, in spawn order, so a test can
/// await a specific task's exit instead of sleeping.
#[derive(Default)]
struct RecordingSpawner(Mutex<Vec<tokio::task::JoinHandle<()>>>);

impl Spawner for RecordingSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        tokio::task::spawn_blocking(task);
    }

    fn spawn(&self, task: Task) -> Joining {
        self.0.lock().unwrap().push(tokio::spawn(task));
        Box::pin(async { Ok(()) })
    }
}

async fn arm(
    folder: &str,
    spawner: &Arc<dyn Spawner>,
) -> tokio::sync::oneshot::Sender<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (_cadence_tx, cadence) = tokio::sync::watch::channel(Cadence::Live);
    let dispatch: RebuildDispatch = Arc::new(|_| Box::pin(async {}));
    let attempt: RebuildAttempt =
        Arc::new(|_, _| Box::pin(async { worker::AttemptOutcome::Completed }));
    start(WatchConfig {
        folder_path: folder.to_string(),
        spawner: spawner.clone(),
        shutdown_rx,
        emit: Arc::new(|_| {}),
        dispatch,
        attempt,
        cadence,
    })
    .await;
    shutdown_tx
}

/// The old watcher task's exit used to shut down and deregister the worker
/// both tasks shared, leaving the live watcher with no worker to enqueue into.
#[tokio::test]
async fn the_old_watchers_exit_leaves_the_re_armed_watchers_worker_registered() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap().to_string_lossy().to_string();
    let spawner = Arc::new(RecordingSpawner::default());

    let first_shutdown = arm(&folder, &(spawner.clone() as Arc<dyn Spawner>)).await;
    // Spawned so far: the worker loop, then the first watcher task.
    let (worker_loop, first_watcher) = {
        let tasks = spawner.0.lock().unwrap();
        assert_eq!(tasks.len(), 2);
        (tasks[0].abort_handle(), tasks[1].abort_handle())
    };
    let second_shutdown = arm(&folder, &(spawner.clone() as Arc<dyn Spawner>)).await;
    let registered = worker::get(&folder).expect("the worker is registered after re-arming");

    let _ = first_shutdown.send(());
    let first_task = spawner.0.lock().unwrap().remove(1);
    first_task.await.unwrap();
    assert!(first_watcher.is_finished());

    let after = worker::get(&folder).expect("the old watcher's exit must not deregister the worker");
    assert!(Arc::ptr_eq(&after, &registered));
    tokio::task::yield_now().await;
    assert!(!worker_loop.is_finished(), "the old watcher's exit must not stop the worker loop");

    // The last watcher out does tear it down.
    let _ = second_shutdown.send(());
    let second_task = spawner.0.lock().unwrap().remove(1);
    second_task.await.unwrap();
    assert!(worker::get(&folder).is_none());
}

/// Runs the first task it is given (the worker loop) and drops every later
/// one unpolled, as a runtime shutting down would the watcher task.
#[derive(Default)]
struct DropsWatcherSpawner(Mutex<Vec<tokio::task::JoinHandle<()>>>);

impl Spawner for DropsWatcherSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        tokio::task::spawn_blocking(task);
    }

    fn spawn(&self, task: Task) -> Joining {
        let mut tasks = self.0.lock().unwrap();
        if tasks.is_empty() {
            tasks.push(tokio::spawn(task));
        }
        Box::pin(async { Ok(()) })
    }
}

/// A watcher task that never runs still owned a claim on the worker; the
/// claim must be released when its future is dropped, or the worker loop and
/// its registry entry outlive a watch that never started.
#[tokio::test]
async fn a_watcher_task_dropped_unpolled_releases_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap().to_string_lossy().to_string();
    let spawner = Arc::new(DropsWatcherSpawner::default());

    let _shutdown = arm(&folder, &(spawner.clone() as Arc<dyn Spawner>)).await;

    assert!(worker::get(&folder).is_none(), "the dropped watcher's claim must deregister the worker");
    let worker_loop = spawner.0.lock().unwrap().remove(0);
    tokio::time::timeout(std::time::Duration::from_secs(5), worker_loop)
        .await
        .expect("the worker loop must be told to exit")
        .unwrap();
}
