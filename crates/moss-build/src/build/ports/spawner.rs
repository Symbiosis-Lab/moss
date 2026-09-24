//! Where background build work runs — the port that replaces direct
//! `tauri::async_runtime::spawn_blocking` calls in the media pipeline.
//!
//! Nothing here names tauri, which is the point: `build/` becomes the open
//! `moss-build` crate, and `check-crate-dag.mjs` rule 2 forbids a path
//! to tauri from it. The tauri-backed implementation lives app-side in
//! `crate::events`, beside `TauriReporter`.
//!
//! **Absence is meaningful.** `BuildServices.spawner` is `None` exactly when
//! there is no tauri runtime to spawn onto — CLI builds and tests — and the
//! media dispatchers read that as "run this work synchronously instead, so the
//! output is complete when the build returns." Before this port existed they
//! asked `event_sink.is_none()`, which gave the same answer for the same reason
//! (`BuildServices` is built from `config.app`: `from_app` sets both, `headless`
//! sets neither) but phrased the question as if it were about reporting. It
//! never was.
//!
//! `spawn` is the async half, and it is NOT read that way: `spawn_process_hooks`
//! and the background comment sync take a `&dyn Spawner` from their caller
//! rather than from `BuildServices`, because both run on a headless `moss build`
//! too — `startup/headless.rs` has a runtime, it just has no shell.
//!
//! ## The invariant this port must not launder
//!
//! One process, one runtime. Implementations MUST dispatch onto the runtime
//! the process already runs — the desktop app's `TauriSpawner` onto
//! `tauri::async_runtime`, [`TokioSpawner`] onto the ambient tokio runtime of
//! a CLI/headless process. Handing the runtime in through a trait makes the
//! leaves stop *naming* a runtime — it does not make a second runtime safe,
//! so never construct one inside an implementation.

/// An async task handed to [`Spawner::spawn`].
pub type Task = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;

/// What [`Spawner::spawn`] hands back: await it to wait for the task, or drop
/// it to fire and forget.
///
/// `Result<(), String>` rather than a runtime-specific join handle, because the
/// one caller that cares — the plugin-install command — turns a panicked
/// process hook into an error the user reads.
pub type Joining = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'static>,
>;

/// Runs work that would otherwise block a build thread.
pub trait Spawner: Send + Sync {
    /// Run `task` on a blocking worker thread. Fire-and-forget: the three media
    /// call sites discard the handle and rely on the folder session's UI-bound
    /// counter to know when the work finished.
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>);

    /// Run `task` on the async runtime.
    fn spawn(&self, task: Task) -> Joining;
}

/// The [`Spawner`] of a process whose one runtime is plain tokio — a headless
/// `moss build` (`startup/headless.rs`) or moss-cli. `tokio::spawn` here IS
/// the one-runtime rule: called from inside the process's only runtime, it
/// lands on that runtime; there is no second one to corrupt anything with.
///
/// Callers must be inside a runtime context (every build/watch call site is —
/// they run under the host's `block_on`).
pub struct TokioSpawner;

impl Spawner for TokioSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        tokio::task::spawn_blocking(task);
    }

    fn spawn(&self, task: Task) -> Joining {
        let handle = tokio::spawn(task);
        Box::pin(async move {
            handle
                .await
                .map_err(|e| format!("background task panicked: {e}"))
        })
    }
}

/// A [`Spawner`] that keeps what [`spawn`](Spawner::spawn) is given instead of
/// running it, so a test decides when the detached seal tail runs.
///
/// `test_host_ports()`'s `NoSpawner` cannot: its `spawn` returns a lazy future
/// that runs the task only when polled, and `run_pipeline`'s detached arm
/// (`exits_after_build: false`) drops the future it gets back without polling it,
/// so under that host the tail never runs at all. `spawn_blocking` runs inline,
/// as `NoSpawner`'s does.
///
/// The `Joining` it hands back is already `Ok`, which is only right for a caller
/// that drops it — the seal tail. One that awaits it to learn the task finished
/// (the `PluginMode::Blocking` process hooks) would be told so without the task
/// having run; drive builds through this with `PluginMode::SlotsOnly`, which
/// starts neither.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct CapturingSpawner {
    tasks: std::sync::Mutex<Vec<Task>>,
}

#[cfg(test)]
impl Spawner for CapturingSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        task();
    }

    fn spawn(&self, task: Task) -> Joining {
        self.tasks.lock().unwrap().push(task);
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
impl CapturingSpawner {
    /// The one task the last build handed over: its detached seal tail.
    ///
    /// Exactly one is asserted rather than assumed. Anything else spawned on the
    /// build's behalf would otherwise be mistaken for the tail and polled in its
    /// place, and the test would drive something that is not the code under test.
    pub(crate) fn take_seal_tail(&self) -> Task {
        let mut tasks = self.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1, "a build must hand the spawner exactly its seal tail, got {} tasks", tasks.len());
        tasks.pop().unwrap()
    }
}
