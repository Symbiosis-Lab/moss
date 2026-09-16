//! Hook-execution state: what the host knows about a plugin hook while it runs.
//!
//! Split out of `commands.rs` (2026-08-28) — a command is a request from the
//! frontend; this is the state three separate readers meet in: the
//! `plugin_message` command writes errors and completions, the engine's host
//! dispatch marks in-flight host calls, and the manager's inactivity watchdog
//! decides from both whether a silent hook is stuck.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::build::cache::Singleflight;

/// Result sent through completion channel when hook finishes
pub struct CompletionResult {
    pub success: bool,
    pub error: Option<String>,
    /// Full hook result including deployment info (for publisher plugins)
    pub result: Option<crate::plugins::types::HookResult>,
}

/// The longest an unbroken run of host calls may hold the inactivity watchdog
/// off: `execute_binary`'s own ceiling (`timeoutMs`, max 600 s). Past it the
/// HOST is the thing that is stuck, and a hung host arm must not disarm the
/// watchdog for the rest of the session — which is what an unbounded reprieve
/// would do, trading a 60 s false kill for an infinite hang.
const HOST_CALL_REPRIEVE: Duration = Duration::from_secs(600);

/// Why one plugin's hook is legitimately silent, and whether that reason has a
/// deadline. Keyed by plugin: the host bridge and the modal both know which
/// plugin, not which hook, and a plugin runs one hook at a time
/// (`executing_hooks`).
///
/// One record rather than a map per reason, because the watchdog asks a single
/// question — is this plugin blocked, and does the block expire — and a reason
/// added as its own map would add another clause to every reader. Not an enum:
/// the reasons overlap by construction. `reject_plugin_secret` IS a host call,
/// and it opens a modal while that call is still in flight, so a plugin is
/// routinely inside both at once.
#[derive(Default)]
struct Blocked {
    /// Host-function calls in flight, and when the unbroken run began (the 0→1
    /// transition). `since` is the run's, not each call's — the question is how
    /// long this plugin has been continuously inside the host, and a plugin
    /// issuing calls back to back is still waiting. Bounded by
    /// [`HOST_CALL_REPRIEVE`]: a hung host arm must not disarm the watchdog for
    /// the rest of the session.
    host_calls: usize,
    host_calls_since: Option<Instant>,
    /// Questions MOSS currently has on screen for this plugin's user — today,
    /// open credential re-prompts. Unbounded, unlike `host_calls`: a modal is a
    /// PERSON being slow, which has no honest deadline, the same reasoning that
    /// makes `check_setup`'s wait indefinite.
    person_waits: usize,
}

impl Blocked {
    /// Is anything still holding the watchdog off, given the host-call bound?
    fn holds_watchdog_off(&self, reprieve: Duration) -> bool {
        self.person_waits > 0 || self.host_calls_in_flight_within(reprieve)
    }

    fn host_calls_in_flight_within(&self, reprieve: Duration) -> bool {
        self.host_calls > 0
            && self.host_calls_since.is_some_and(|since| since.elapsed() < reprieve)
    }

    fn is_idle(&self) -> bool {
        self.host_calls == 0 && self.person_waits == 0
    }
}

/// Shared state for tracking plugin hook execution
///
/// This state is used by the `plugin_message` command to collect errors
/// and by `manager.rs` to wait for completion. Clone is field-wise `Arc::clone` — a second handle, never a second state.
#[derive(Clone)]
pub struct PluginHookState {
    /// Collected errors per (plugin_name, hook_name)
    errors: Arc<Mutex<HashMap<(String, String), Vec<(String, bool)>>>>,
    /// Oneshot channels for completion signals per (plugin_name, hook_name)
    completion_senders: Arc<Mutex<HashMap<(String, String), oneshot::Sender<CompletionResult>>>>,
    /// Last activity timestamp per (plugin_name, hook_name) for activity-based timeout
    last_activity: Arc<Mutex<HashMap<(String, String), std::time::Instant>>>,
    /// Which tasks of a (plugin_name, hook_name) are currently `Awaiting`.
    ///
    /// Task 2 (watchdog fix): when a task enters `Awaiting` (e.g., the matters
    /// syndicate hook waiting for the user to publish in the editor), the 60 s
    /// inactivity watchdog must NOT fire — the hook is legitimately paused waiting
    /// for the user, not stuck.
    ///
    /// The entry is a SET of task ids, not a flag, because the question the
    /// watchdog asks is "is anyone still waiting on the person" — a hook whose
    /// question was answered must go back under the clock. A flag could only be
    /// cleared at hook teardown, which exempted everything the hook did after
    /// the question (github's repo form disarmed the watchdog for the whole
    /// remaining push). An emptied entry is removed, so "this hook is waiting"
    /// is exactly "the map has a key for it".
    awaiting_hooks: Arc<Mutex<HashMap<(String, String), std::collections::HashSet<u64>>>>,
    /// Currently-executing hook name per plugin.
    ///
    /// Phase 2 watchdog fix (B2 interim): the matters `process` hook creates its
    /// task with `hook: "import"` so `apply_awaiting("matters","import",…)` fires,
    /// but the watchdog checks `is_hook_awaiting("matters","process")` —
    /// a key mismatch that kills the hook at 60 s mid-login.
    ///
    /// Fix (option a): `apply_awaiting` consults this map and additionally marks
    /// `(plugin, executing_hook_name)` as awaiting, so the watchdog's check
    /// against the EXECUTING hook_name succeeds regardless of the task's UI
    /// `hook` label. Populated by `register_executing_hook` (called from
    /// `execute_plugin_javascript` before the select! watchdog loop) and cleared
    /// by `unregister_executing_hook` (called at teardown). Does NOT change the
    /// plugin-visible task surface — `hook: "import"` still routes correctly.
    executing_hooks: Arc<Mutex<HashMap<String, String>>>,
    /// Why each plugin is legitimately silent right now — see [`Blocked`]. A
    /// slow host arm (github deploy's `execute_binary` git push, a large
    /// upload) and a modal moss put on screen both produce no plugin-side
    /// signal, so the 60 s inactivity watchdog killed hooks whose work WAS
    /// advancing; that is the reason first-party plugins ship fake heartbeats.
    /// An idle record is removed, so a key here IS a blocked plugin.
    blocked: Arc<Mutex<HashMap<String, Blocked>>>,
    /// Singleflight guard for plugin hook dispatch.
    ///
    /// Prevents concurrent dispatch of the same hook to the JS runtime.
    /// When the same plugin::hook is already in-flight (e.g., install triggers
    /// process hook while build also triggers it), the second caller shares
    /// the first's result instead of dispatching again.
    ///
    /// Key format: "{plugin_name}::{hook_name}"
    ///
    /// This solves the __MOSS_INTERNAL_CONTEXT__ clobbering bug: the JS runtime
    /// has a single shared context global, so only one hook execution per plugin
    /// can safely run at a time.
    pub in_flight_hooks: Arc<Singleflight<Result<(), String>>>,
}

impl Default for PluginHookState {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHookState {
    pub fn new() -> Self {
        Self {
            errors: Arc::new(Mutex::new(HashMap::new())),
            completion_senders: Arc::new(Mutex::new(HashMap::new())),
            last_activity: Arc::new(Mutex::new(HashMap::new())),
            awaiting_hooks: Arc::new(Mutex::new(HashMap::new())),
            executing_hooks: Arc::new(Mutex::new(HashMap::new())),
            blocked: Arc::new(Mutex::new(HashMap::new())),
            in_flight_hooks: Arc::new(Singleflight::new()),
        }
    }

    /// Add an error for a plugin hook
    pub fn add_error(&self, plugin_name: &str, hook_name: &str, error: String, fatal: bool) {
        if let Ok(mut map) = self.errors.lock() {
            map.entry((plugin_name.to_string(), hook_name.to_string()))
                .or_default()
                .push((error, fatal));
        }
    }

    /// Get and clear errors for a plugin hook
    pub fn take_errors(&self, plugin_name: &str, hook_name: &str) -> Vec<(String, bool)> {
        if let Ok(mut map) = self.errors.lock() {
            map.remove(&(plugin_name.to_string(), hook_name.to_string()))
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Clear all state for a plugin hook
    pub fn clear(&self, plugin_name: &str, hook_name: &str) {
        let key = (plugin_name.to_string(), hook_name.to_string());
        if let Ok(mut errors) = self.errors.lock() {
            errors.remove(&key);
        }
        if let Ok(mut senders) = self.completion_senders.lock() {
            senders.remove(&key);
        }
        if let Ok(mut activity) = self.last_activity.lock() {
            activity.remove(&key);
        }
        if let Ok(mut awaiting) = self.awaiting_hooks.lock() {
            awaiting.remove(&key);
        }
    }

    /// Register the currently-executing hook for a plugin.
    ///
    /// Called from `execute_plugin_javascript` (manager.rs) before the
    /// inactivity-watchdog select! loop, so that `apply_awaiting` can look up
    /// the executing hook name and mark it awaiting even when the task's UI
    /// `hook` label (e.g. "import") differs from the executing hook name
    /// (e.g. "process"). See `executing_hooks` doc-comment for the full
    /// rationale (Phase 2 B2 interim watchdog key-mismatch fix).
    pub fn register_executing_hook(&self, plugin_name: &str, hook_name: &str) {
        if let Ok(mut map) = self.executing_hooks.lock() {
            map.insert(plugin_name.to_string(), hook_name.to_string());
        }
    }

    /// Clear the executing-hook registration for a plugin.
    ///
    /// Called from `execute_plugin_javascript` (manager.rs) at teardown —
    /// after the select! loop exits (success, timeout, or cancel).
    pub fn unregister_executing_hook(&self, plugin_name: &str) {
        if let Ok(mut map) = self.executing_hooks.lock() {
            map.remove(plugin_name);
        }
    }

    /// Record whether a task of the running hook is waiting on the person.
    ///
    /// The one door both lifecycle bridges come through (`awaiting` is the
    /// lifecycle's own `is_awaiting()`): while any task of a hook is waiting the
    /// inactivity watchdog leaves it alone, and the moment the last one stops
    /// the hook is back under the 60 s clock.
    ///
    /// The watchdog checks the EXECUTING hook name, but the task may report a
    /// different UI hook label (matters `process` reports `hook:"import"`). So we
    /// key the awaiting flag on the executing hook (resolved from
    /// `executing_hooks`), falling back to the passed label only when no hook is
    /// registered (out-of-hook callers). Keying on a single resolved hook lets
    /// teardown's `clear(plugin, executing_hook)` remove exactly what was
    /// inserted — no stale UI-label entry leaks into `awaiting_hooks`.
    ///
    /// A report with no task id is still proof of life. Only `Started` arrives
    /// without one (the registry mints the id), so there is no task to add or
    /// remove — but the hook just spoke, and the clock restarts below either
    /// way. Skipping the call for those would let a webview plugin that works
    /// quietly for a minute and then calls `startTask()` be killed by the
    /// watchdog; on QuickJS the `HostCallGuard` masks that, on the Tauri command
    /// arm nothing does.
    ///
    /// Private so the one door stays one: the two bridges reach it through
    /// `record_lifecycle`, and only this module's own tests may drive the bool
    /// directly.
    fn apply_awaiting(
        &self,
        plugin_name: &str,
        hook_name: &str,
        task: Option<u64>,
        awaiting: bool,
    ) {
        // Resolve + release the executing_hooks lock BEFORE taking awaiting_hooks
        // (no nested locks).
        let key_hook = self.executing_hook_key(plugin_name, hook_name);
        let key = (plugin_name.to_string(), key_hook.clone());
        if let Some(task) = task {
            if let Ok(mut map) = self.awaiting_hooks.lock() {
                if awaiting {
                    map.entry(key).or_default().insert(task);
                } else if let Some(tasks) = map.get_mut(&key) {
                    tasks.remove(&task);
                    if tasks.is_empty() {
                        map.remove(&key);
                    }
                }
            }
        }
        // Reset the clock either way: entering the wait so the watchdog's first
        // tick after it does not trip, and leaving it so the 60 s starts at the
        // answer rather than before the question.
        self.update_activity(plugin_name, &key_hook);
    }

    /// Record what a lifecycle report means for the watchdog.
    ///
    /// Both engine bridges call this and nothing else, so neither gets to
    /// re-derive which reports count as waiting — `is_awaiting` is the one
    /// owner and this is the one door. The bridges used to pass a bool they
    /// computed themselves, which is how they drifted apart once already.
    ///
    /// `attended` is whether a person could answer the question. Unattended
    /// (a headless build) a question is recorded as NOT waiting, so the
    /// watchdog cuts the hook off after the inactivity timeout instead of
    /// holding a CI job on a prompt nobody will ever see; the report still
    /// restamps the clock either way, because it is the hook speaking.
    pub fn record_lifecycle(
        &self,
        plugin_name: &str,
        hook_name: &str,
        task: Option<u64>,
        lifecycle: &crate::plugins::runtime::portable::PluginTaskLifecycle,
        attended: bool,
    ) {
        self.apply_awaiting(plugin_name, hook_name, task, attended && lifecycle.is_awaiting());
    }

    /// The hook name the watchdog keys on for this plugin: the EXECUTING hook,
    /// falling back to `hook_name` when nothing is registered (out-of-hook
    /// callers). Anything that wants to reach the watchdog's key resolves here.
    fn executing_hook_key(&self, plugin_name: &str, hook_name: &str) -> String {
        self.executing_hooks
            .lock()
            .ok()
            .and_then(|map| map.get(plugin_name).cloned())
            .unwrap_or_else(|| hook_name.to_string())
    }

    /// Mark the plugin as blocked on a host-function call until the returned
    /// guard drops. See [`Blocked`].
    pub fn begin_host_call(&self, plugin_name: &str) -> HostCallGuard {
        if let Ok(mut map) = self.blocked.lock() {
            let block = map.entry(plugin_name.to_string()).or_default();
            block.host_calls += 1;
            block.host_calls_since.get_or_insert_with(Instant::now);
        }
        HostCallGuard { state: self.clone(), plugin_name: plugin_name.to_string() }
    }

    /// Whether the inactivity watchdog must leave a silent hook alone.
    ///
    /// Three ways a hook can be silent and healthy:
    /// - **Awaiting** — legitimately paused ON the user (the matters editor).
    ///   `apply_awaiting` also resets `last_activity`, so the first watchdog tick
    ///   after entry is safe too (Task 2 watchdog fix).
    /// - **A host call in flight** — the HOST is the slow one (a git push, a
    ///   large upload). Killing the hook would abandon work that is advancing,
    ///   which is why plugins used to emit fake heartbeats to stay alive. Bounded
    ///   by [`HOST_CALL_REPRIEVE`]: a hung host arm gets the watchdog back.
    /// - **A question moss is asking the person** — a credential re-prompt is on
    ///   screen. Unbounded, like `Awaiting` and for the same reason.
    pub fn is_making_progress(&self, plugin_name: &str, hook_name: &str) -> bool {
        self.is_hook_awaiting(plugin_name, hook_name)
            || self.with_block(plugin_name, |b| b.holds_watchdog_off(HOST_CALL_REPRIEVE))
    }

    /// Park the plugin on a question moss is asking, until the guard drops.
    pub fn begin_person_wait(&self, plugin_name: &str) -> PersonWaitGuard {
        if let Ok(mut map) = self.blocked.lock() {
            map.entry(plugin_name.to_string()).or_default().person_waits += 1;
        }
        PersonWaitGuard { state: self.clone(), plugin_name: plugin_name.to_string() }
    }

    /// True while this plugin has been continuously inside host calls for less
    /// than `reprieve`. Public for the watchdog's own tests, which cannot wait
    /// out the real bound.
    pub fn has_in_flight_host_call_within(&self, plugin_name: &str, reprieve: Duration) -> bool {
        self.with_block(plugin_name, |b| b.host_calls_in_flight_within(reprieve))
    }

    /// Read one plugin's block record. A poisoned or absent entry answers as an
    /// unblocked plugin, which is the fail-safe direction: the watchdog stays
    /// armed rather than being disarmed by a lock error.
    fn with_block(&self, plugin_name: &str, f: impl FnOnce(&Blocked) -> bool) -> bool {
        self.blocked
            .lock()
            .ok()
            .and_then(|map| map.get(plugin_name).map(f))
            .unwrap_or(false)
    }

    /// Drop one reason for a plugin's block, and forget the record once nothing
    /// is holding it. Then restart the hook's silence clock, so the 60 s is
    /// measured from the answer rather than from the question.
    fn end_block(&self, plugin_name: &str, release: impl FnOnce(&mut Blocked)) {
        if let Ok(mut map) = self.blocked.lock() {
            if let Some(block) = map.get_mut(plugin_name) {
                release(block);
                if block.is_idle() {
                    map.remove(plugin_name);
                }
            }
        }
        self.restart_activity_clock(plugin_name);
    }

    /// Restamp the clock under the hook the watchdog keys on. Nothing executing
    /// means nothing to restamp.
    fn restart_activity_clock(&self, plugin_name: &str) {
        let hook = self
            .executing_hooks
            .lock()
            .ok()
            .and_then(|map| map.get(plugin_name).cloned());
        if let Some(hook) = hook {
            self.update_activity(plugin_name, &hook);
        }
    }

    /// Returns true iff the hook is currently in the `Awaiting` state.
    /// The inactivity watchdog must not fire while this is true.
    pub fn is_hook_awaiting(&self, plugin_name: &str, hook_name: &str) -> bool {
        let key = (plugin_name.to_string(), hook_name.to_string());
        // An emptied entry is removed by `apply_awaiting`, so a key IS a waiter.
        self.awaiting_hooks
            .lock()
            .ok()
            .is_some_and(|map| map.contains_key(&key))
    }

    /// Initialize activity timestamp for a plugin hook (called when hook execution starts)
    pub fn init_activity(&self, plugin_name: &str, hook_name: &str) {
        self.update_activity(plugin_name, hook_name);
    }

    /// Update activity timestamp for a plugin hook (called on progress messages)
    pub fn update_activity(&self, plugin_name: &str, hook_name: &str) {
        if let Ok(mut map) = self.last_activity.lock() {
            map.insert(
                (plugin_name.to_string(), hook_name.to_string()),
                std::time::Instant::now(),
            );
        }
    }

    /// Get last activity timestamp for a plugin hook
    /// Returns current time if no activity recorded (safe default)
    pub fn get_last_activity(&self, plugin_name: &str, hook_name: &str) -> std::time::Instant {
        self.last_activity
            .lock()
            .ok()
            .and_then(|map| map.get(&(plugin_name.to_string(), hook_name.to_string())).copied())
            .unwrap_or_else(std::time::Instant::now)
    }

    /// Store a completion sender for a plugin hook (called by execute_hook before waiting)
    pub fn set_completion_sender(
        &self,
        plugin_name: &str,
        hook_name: &str,
        sender: oneshot::Sender<CompletionResult>,
    ) {
        if let Ok(mut map) = self.completion_senders.lock() {
            map.insert(
                (plugin_name.to_string(), hook_name.to_string()),
                sender,
            );
        }
    }

    /// Send completion result through channel (called by plugin_message on Complete)
    pub fn send_completion(
        &self,
        plugin_name: &str,
        hook_name: &str,
        result: CompletionResult,
    ) {
        if let Ok(mut map) = self.completion_senders.lock() {
            if let Some(sender) = map.remove(&(plugin_name.to_string(), hook_name.to_string())) {
                // Ignore send error - receiver might have been dropped
                let _ = sender.send(result);
            }
        }
    }
}

/// Held for the lifetime of one host-function call issued by a plugin.
///
/// On drop it decrements the count and restamps the hook's activity clock, so
/// the 60 s inactivity timeout measures silence AFTER the host finished rather
/// than during it.
pub struct HostCallGuard {
    state: PluginHookState,
    plugin_name: String,
}

impl Drop for HostCallGuard {
    fn drop(&mut self) {
        self.state.end_block(&self.plugin_name, |block| {
            block.host_calls = block.host_calls.saturating_sub(1);
            if block.host_calls == 0 {
                block.host_calls_since = None;
            }
            // While a run continues `since` deliberately stays put, so
            // back-to-back calls cannot renew the reprieve forever.
        });
    }
}

/// Held for as long as one of moss's own questions is on screen for this
/// plugin. On drop the count falls and the hook's activity clock restarts, so
/// the 60 s silence is measured from the ANSWER rather than from the question.
pub struct PersonWaitGuard {
    state: PluginHookState,
    plugin_name: String,
}

impl Drop for PersonWaitGuard {
    fn drop(&mut self) {
        self.state.end_block(&self.plugin_name, |block| {
            block.person_waits = block.person_waits.saturating_sub(1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hook parked on a question MOSS asked must survive past the host-call
    /// reprieve: the modal is still on screen and the person has not answered.
    #[test]
    fn a_question_on_screen_holds_the_watchdog_off_without_a_deadline() {
        let state = PluginHookState::default();
        state.register_executing_hook("ipfs", "deploy");
        assert!(!state.is_making_progress("ipfs", "deploy"));

        let waiting = state.begin_person_wait("ipfs");
        assert!(state.is_making_progress("ipfs", "deploy"));
        // The host-call reprieve is what a credential modal would outlive; this
        // wait is not measured by it at all.
        assert!(!state.has_in_flight_host_call_within("ipfs", HOST_CALL_REPRIEVE));

        drop(waiting);
        assert!(
            !state.is_making_progress("ipfs", "deploy"),
            "the answer puts the hook back under the clock"
        );
    }

    /// A question asked of one plugin's user says nothing about another plugin.
    #[test]
    fn a_question_only_covers_its_own_plugin() {
        let state = PluginHookState::default();
        state.register_executing_hook("github", "deploy");
        let _waiting = state.begin_person_wait("ipfs");
        assert!(!state.is_making_progress("github", "deploy"));
    }

    /// A hook that is silent while the HOST runs its call must not be killed.
    /// The watchdog's 60 s constant makes the real loop untestable in a unit
    /// test; this pins the rule the loop asks about instead.
    #[test]
    fn an_in_flight_host_call_holds_the_watchdog_off() {
        let state = PluginHookState::default();
        state.register_executing_hook("github", "deploy");
        assert!(
            !state.is_making_progress("github", "deploy"),
            "a silent hook with no host call and no Awaiting is a timeout candidate"
        );

        let busy = state.begin_host_call("github");
        assert!(
            state.is_making_progress("github", "deploy"),
            "while the host runs the plugin's call, the plugin is not stuck"
        );

        drop(busy);
        assert!(
            !state.is_making_progress("github", "deploy"),
            "the reprieve ends with the call — the clock restarts, it does not stop"
        );
    }

    /// Concurrent host calls: the reprieve lasts until the LAST one ends, so a
    /// plugin with two calls in flight is not killed when the first returns.
    #[test]
    fn concurrent_host_calls_hold_off_until_the_last_returns() {
        let state = PluginHookState::default();
        state.register_executing_hook("github", "deploy");
        let first = state.begin_host_call("github");
        let second = state.begin_host_call("github");
        drop(first);
        assert!(state.is_making_progress("github", "deploy"), "one call still running");
        drop(second);
        assert!(!state.is_making_progress("github", "deploy"));
    }

    /// A host call by one plugin says nothing about another plugin's hook.
    #[test]
    fn a_host_call_only_covers_its_own_plugin() {
        let state = PluginHookState::default();
        let _busy = state.begin_host_call("github");
        assert!(!state.is_making_progress("matters", "syndicate"));
    }

    /// A host arm that never returns must not disarm the watchdog for good: past
    /// the reprieve the hook is a timeout candidate again, hung host or not.
    #[test]
    fn the_reprieve_is_bounded() {
        let state = PluginHookState::default();
        let _busy = state.begin_host_call("github");
        assert!(state.has_in_flight_host_call_within("github", Duration::from_secs(600)));
        assert!(
            !state.has_in_flight_host_call_within("github", Duration::ZERO),
            "a run older than the reprieve stops counting as progress"
        );
    }

    /// The activity clock restarts when the call ENDS: a hook that spent 59 s
    /// inside one host call must not be killed one second after it returns.
    #[test]
    fn the_clock_restarts_when_the_call_returns() {
        let state = PluginHookState::default();
        state.register_executing_hook("github", "deploy");
        state.init_activity("github", "deploy");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let before = state.get_last_activity("github", "deploy");
        drop(state.begin_host_call("github"));
        assert!(
            state.get_last_activity("github", "deploy") > before,
            "dropping the guard restamps activity, so the timeout measures silence \
             after the host finished"
        );
    }

    // ── watchdog key-mismatch fix (Phase 2 B2 interim) ───────────────────────

    /// The matters `process` hook creates its task with `hook:"import"`.
    /// When `task.awaiting()` fires, `apply_awaiting("matters","import",…)` is
    /// called, but the watchdog in `execute_plugin_javascript` checks
    /// `is_hook_awaiting("matters","process")` — a key mismatch that caused
    /// the hook to be killed at 60 s mid-login.
    ///
    /// Fix (option a): `register_executing_hook("matters","process")` is called
    /// before the watchdog select! loop; `apply_awaiting` then also marks
    /// `("matters","process")` as awaiting. This test asserts that contract.
    ///
    /// The test exercises the EXACT bug path:
    /// 1. Register the executing hook (as `execute_plugin_javascript` does).
    /// 2. Simulate `task.awaiting()` by calling `apply_awaiting` with the UI label.
    /// 3. Assert the watchdog check on the EXECUTING hook name returns true.
    /// 4. Assert the watchdog check still returns false when NOT registered —
    ///    verifying that the fix is conditional on the executing-hook registration.
    #[test]
    fn watchdog_skips_process_hook_when_import_task_reports_awaiting() {
        let state = PluginHookState::new();

        // Step 1: simulate execute_plugin_javascript registering "process" as
        // the currently-executing hook for the "matters" plugin.
        state.register_executing_hook("matters", "process");

        // Step 2: simulate the JS task.awaiting() call — the matters process
        // hook reports its task with hook:"import", so apply_awaiting is called
        // with "import" (the UI label), NOT "process" (the executing hook name).
        state.apply_awaiting("matters", "import", Some(1), true);

        // Step 3: the watchdog's check — it uses the EXECUTING hook name
        // ("process"). This MUST return true now that the fix is in place.
        assert!(
            state.is_hook_awaiting("matters", "process"),
            "watchdog must see the executing hook ('process') as awaiting when its \
             task reports awaiting under a different UI label ('import'); without \
             register_executing_hook + apply_awaiting propagation, this returns false \
             and the watchdog kills the hook at 60 s mid-login"
        );

        // The UI label ("import") must NOT be marked — we key only on the
        // resolved executing hook, so teardown's clear("matters","process")
        // removes exactly what was inserted (no stale UI-label entry leaks).
        assert!(
            !state.is_hook_awaiting("matters", "import"),
            "UI label ('import') must NOT leak into awaiting_hooks — only the \
             executing hook ('process') is keyed, so clear() can fully remove it"
        );

        // Step 4: control — without registration, the propagation does NOT fire.
        // A fresh state with only the UI label set must NOT mark the executing hook.
        let state2 = PluginHookState::new();
        // NOTE: intentionally do NOT call register_executing_hook here.
        state2.apply_awaiting("matters", "import", Some(1), true);
        // Without registration, the executing hook is unknown — only the UI label is marked.
        assert!(
            state2.is_hook_awaiting("matters", "import"),
            "UI label still marked without registration"
        );
        assert!(
            !state2.is_hook_awaiting("matters", "process"),
            "executing hook ('process') must NOT be marked when registration was skipped"
        );
    }

    /// After `unregister_executing_hook`, a subsequent `apply_awaiting` for the
    /// same plugin must NOT propagate to the previously-executing hook. This
    /// ensures teardown is clean and the next hook invocation starts fresh.
    #[test]
    fn unregister_clears_executing_hook_propagation() {
        let state = PluginHookState::new();

        state.register_executing_hook("matters", "process");
        state.unregister_executing_hook("matters");

        // After unregister, apply_awaiting with "import" should NOT mark "process".
        state.apply_awaiting("matters", "import", Some(1), true);
        assert!(
            !state.is_hook_awaiting("matters", "process"),
            "after unregister, apply_awaiting must not propagate to the old executing hook"
        );
    }

    /// When the task's UI hook label matches the executing hook name
    /// (e.g., syndicate hook reports `hook:"syndicate"`, which already worked),
    /// `apply_awaiting` must not double-insert or cause any issue.
    #[test]
    fn apply_awaiting_idempotent_when_labels_match() {
        let state = PluginHookState::new();

        state.register_executing_hook("matters", "syndicate");
        state.apply_awaiting("matters", "syndicate", Some(1), true);

        // Both checks must succeed (no regression for the already-working path).
        assert!(state.is_hook_awaiting("matters", "syndicate"));
    }

    /// A question the user answered must put the hook back under the watchdog.
    /// github's repo-name form used to leave the flag set for the rest of the
    /// deploy, so a push that hung after the form hung forever.
    #[test]
    fn answering_the_question_arms_the_watchdog_again() {
        let state = PluginHookState::new();
        state.register_executing_hook("github", "deploy");

        state.apply_awaiting("github", "deploy", Some(7), true);
        assert!(state.is_hook_awaiting("github", "deploy"));

        state.apply_awaiting("github", "deploy", Some(7), false);
        assert!(
            !state.is_hook_awaiting("github", "deploy"),
            "nothing is waiting on the person any more"
        );
    }

    /// A report with no task id — only `Started`, whose id the registry mints —
    /// records no waiter but is still proof of life. Skipping the call for it
    /// once let a webview plugin that worked quietly for a minute and then
    /// called `startTask()` be killed by the inactivity watchdog.
    #[test]
    fn a_report_with_no_task_id_is_still_proof_of_life() {
        let state = PluginHookState::new();
        state.register_executing_hook("github", "deploy");
        state.apply_awaiting("github", "deploy", Some(7), true);

        let before = state.get_last_activity("github", "deploy");
        std::thread::sleep(std::time::Duration::from_millis(2));
        state.apply_awaiting("github", "deploy", None, false);

        assert!(
            state.get_last_activity("github", "deploy") > before,
            "a task starting is a heartbeat"
        );
        assert!(
            state.is_hook_awaiting("github", "deploy"),
            "and it speaks for no task, so task 7 is still asking"
        );
    }

    /// The Tauri-command bridge's half of the watchdog contract.
    ///
    /// `host_fns.rs` has `every_lifecycle_report_reaches_the_watchdog` for the
    /// QuickJS arm, where a `HostCallGuard` would mask a regression anyway.
    /// This arm has no such guard, and had no test either: reverting the bridge
    /// to fire only on `Awaiting` passed the whole suite. Every variant must
    /// restamp the clock; only `Awaiting` may record a task.
    #[test]
    fn every_lifecycle_report_restamps_the_watchdog_clock() {
        use crate::plugins::runtime::portable::PluginTaskLifecycle as L;

        let started = L::Started {
            label: "Deploying".to_string(),
            has_progress: false,
            cancellable: false,
            job: None,
        };
        let reports = [
            (None, &started),
            (Some(1), &L::Progress { fraction: None, message: None }),
            (
                Some(1),
                &L::Awaiting {
                    directive: "open:https://example.com".to_string(),
                    escape: "cancel".to_string(),
                },
            ),
            (
                Some(1),
                &L::Succeeded { receipt: None, advisories: Vec::new(), amount: None },
            ),
            (
                Some(1),
                &L::Failed {
                    error: "no".to_string(),
                    advisories: Vec::new(),
                    recoverable: false,
                },
            ),
            (Some(1), &L::Cancelled),
        ];

        for (task, lifecycle) in reports {
            let state = PluginHookState::new();
            state.register_executing_hook("github", "deploy");
            // Seed the clock: `get_last_activity` answers `Instant::now()` for a
            // key it has never seen (the watchdog fails open), so without a real
            // stamp to move, every assertion below would pass vacuously.
            state.update_activity("github", "deploy");
            let before = state.get_last_activity("github", "deploy");
            std::thread::sleep(std::time::Duration::from_millis(2));

            state.record_lifecycle("github", "deploy", task, lifecycle, true);

            assert!(
                state.get_last_activity("github", "deploy") > before,
                "{lifecycle:?} is a report, so it is proof of life"
            );
            assert_eq!(
                state.is_hook_awaiting("github", "deploy"),
                lifecycle.is_awaiting(),
                "{lifecycle:?} must record exactly what is_awaiting says"
            );
        }
    }

    /// One task ending does not speak for another that is still asking.
    #[test]
    fn a_hook_waits_while_any_of_its_tasks_still_asks() {
        let state = PluginHookState::new();
        state.register_executing_hook("matters", "process");

        state.apply_awaiting("matters", "process", Some(1), true);
        state.apply_awaiting("matters", "process", Some(2), true);
        state.apply_awaiting("matters", "process", Some(1), false);

        assert!(state.is_hook_awaiting("matters", "process"));
        state.apply_awaiting("matters", "process", Some(2), false);
        assert!(!state.is_hook_awaiting("matters", "process"));
    }
}
