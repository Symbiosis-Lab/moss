//! The limits plugin code runs under: memory for all plugins of one engine
//! together, and how long code may run without pausing. A pause is control
//! coming back to moss: a request or file call answering, a timer firing, a
//! hook or event being handed in. The clock restarts only there, never on
//! something JS does itself, so `for (;;) invoke(..)` without `await` still
//! trips it.

use rquickjs::allocator::{Allocator, RustAllocator};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    pub memory: usize,
    pub pause: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Self { memory: 256 << 20, pause: Duration::from_secs(10) }
    }
}

thread_local! {
    /// When the JS on this thread's stack was first seen running since the
    /// last pause. Started by the interrupt handler, which runs only while JS does.
    static SLICE_FROM: Cell<Option<Instant>> = const { Cell::new(None) };
    /// How many times the pause limit has stopped code. A count, as
    /// [`REFUSALS`] is: a failure is a pause only if a trip came while its code ran.
    static TRIPS: Cell<u64> = const { Cell::new(0) };
    /// How many allocations [`Capped`] has refused. A count, not a flag: code
    /// compares it across what it ran, so a refusal is never blamed on code
    /// that ran later.
    static REFUSALS: Cell<u64> = const { Cell::new(0) };
    /// Woken by a pause trip. Code resumed after an `await` that is stopped never
    /// settles its promise, so its hook learns of the stop only by being polled.
    static TRIPPED: Rc<Notify> = Rc::new(Notify::new());
}

/// `fut`, polled again whenever the pause limit stops code, so a hook whose
/// code was stopped after an `await` sees the stop instead of waiting forever.
/// Every waiting hook is polled; only the stopped one holds the interrupt.
pub(crate) async fn polled_again_on_trips<F: std::future::Future>(fut: F) -> F::Output {
    let trips = TRIPPED.with(Rc::clone);
    let mut fut = std::pin::pin!(fut);
    loop {
        let tripped = trips.notified();
        let mut tripped = std::pin::pin!(tripped);
        tripped.as_mut().enable();
        tokio::select! {
            biased;
            r = &mut fut => return r,
            _ = tripped => {}
        }
    }
}

/// Control is back in moss: whatever JS runs next starts a fresh slice.
pub(crate) fn paused() {
    SLICE_FROM.with(|c| c.set(None));
}

/// The interrupt handler's check: true once the JS on the stack has run
/// `limit` without a pause.
pub(crate) fn over_pause(limit: Duration) -> bool {
    let now = Instant::now();
    let from = SLICE_FROM.with(|c| {
        let from = c.get().unwrap_or(now);
        c.set(Some(from));
        from
    });
    if now.duration_since(from) < limit {
        return false;
    }
    TRIPS.with(|t| t.set(t.get() + 1));
    TRIPPED.with(|n| n.notify_waiters());
    paused();
    true
}

pub(crate) fn trips() -> u64 {
    TRIPS.with(Cell::get)
}

/// Whether `thrown` is moss's interrupt. Plugin code can neither catch nor
/// throw one, so only the code that was stopped ever fails with it.
pub(crate) fn is_interrupt(thrown: &rquickjs::Value<'_>) -> bool {
    // SAFETY: reads a flag on a live value's object header.
    unsafe { rquickjs::qjs::JS_IsUncatchableError(thrown.as_raw()) }
}

pub(crate) fn pause_message(plugin: &str, limit: Duration) -> String {
    format!("{plugin} ran for {} seconds without pausing and was stopped.", limit.as_secs())
}

/// The engine's allocator. It holds the memory cap itself, rather than QuickJS's
/// own limit, so running out is known by the refusal and never guessed from an
/// error's text, which plugin code can write.
pub(crate) struct Capped {
    limit: usize,
    used: usize,
}

impl Capped {
    pub(crate) fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }

    fn admit(&self, freed: usize, size: usize) -> bool {
        let fits = self.used.saturating_sub(freed).saturating_add(size) <= self.limit;
        if !fits {
            REFUSALS.with(|r| r.set(r.get() + 1));
        }
        fits
    }

    fn counted(&mut self, ptr: *mut u8) -> *mut u8 {
        if !ptr.is_null() {
            self.used += unsafe { RustAllocator::usable_size(ptr) };
        }
        ptr
    }
}

unsafe impl Allocator for Capped {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if !self.admit(0, size) {
            return std::ptr::null_mut();
        }
        let ptr = RustAllocator.alloc(size);
        self.counted(ptr)
    }

    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        match count.checked_mul(size) {
            Some(total) if self.admit(0, total) => {
                let ptr = RustAllocator.calloc(count, size);
                self.counted(ptr)
            }
            _ => std::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        self.used -= RustAllocator::usable_size(ptr);
        RustAllocator.dealloc(ptr);
    }

    unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        let old = RustAllocator::usable_size(ptr);
        if !self.admit(old, new_size) {
            return std::ptr::null_mut();
        }
        let moved = RustAllocator.realloc(ptr, new_size);
        if moved.is_null() {
            return moved;
        }
        self.used -= old;
        self.counted(moved)
    }

    unsafe fn usable_size(ptr: *mut u8) -> usize {
        RustAllocator::usable_size(ptr)
    }
}

pub(crate) fn refusals() -> u64 {
    REFUSALS.with(Cell::get)
}

/// Whether an allocation was refused since `seen`, which moves up to now.
pub(crate) fn refused_since(seen: &mut u64) -> bool {
    let now = refusals();
    std::mem::replace(seen, now) != now
}

pub(crate) fn memory_message(plugin: &str, limit: usize) -> String {
    format!("{plugin} ran out of memory ({} MB) and was stopped.", limit >> 20)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::QuickJsEngine;
    use crate::plugins::hook_state::AskedBy;

    const LIMIT: Duration = Duration::from_millis(300);

    fn engine() -> QuickJsEngine {
        let (engine, mut rx) = QuickJsEngine::with_budgets(None, Budgets { pause: LIMIT, ..Default::default() });
        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let _ = req.reply.send(Err("no cmd".into()));
            }
        });
        engine
    }

    async fn run(engine: &QuickJsEngine, plugin: &str, body: &str) -> Result<String, String> {
        run_with_top(engine, plugin, "", body).await
    }

    /// `top` runs once, when the plugin's Context is made.
    async fn run_with_top(engine: &QuickJsEngine, plugin: &str, top: &str, body: &str) -> Result<String, String> {
        let bundle = format!(r#""use strict"; {top}; var P = (() => ({{ async process(ctx) {{ {body} }} }}))();"#);
        tokio::time::timeout(Duration::from_secs(10), engine.dispatch_hook_raw(plugin, &bundle, "P", "process", "{}", "", ""))
            .await
            .expect("the hook must end")
    }

    /// One body per plugin, so a later run reuses the Context: `fresh` says
    /// whether it was made anew. `{0}` runs only in a fresh one.
    const FRESH: &str = "const fresh = globalThis.mark === undefined; globalThis.mark = 1; if (fresh) { {0} } return { fresh };";

    async fn fresh(engine: &QuickJsEngine, plugin: &str, setup: &str) -> bool {
        let out = run(engine, plugin, &FRESH.replace("{0}", setup)).await.unwrap();
        out.contains("\"fresh\":true")
    }

    const BUSY: &str = "const busy = (ms) => { const t = Date.now(); while (Date.now() - t < ms) {} };";

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn code_that_never_pauses_is_stopped_and_its_plugin_runs_again() {
        let engine = engine();
        let err = run(&engine, "p", "while (true) {}").await.unwrap_err();
        assert_eq!(err, pause_message("p", LIMIT));
        let err = run(&engine, "p", "for (;;) { __TAURI__.core.invoke('x').catch(() => {}); }").await.unwrap_err();
        assert_eq!(err, pause_message("p", LIMIT), "a call it never waits for is not a pause");
        assert!(run(&engine, "p", "return { ok: true };").await.unwrap().contains("true"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn code_that_stops_pausing_after_an_await_is_stopped() {
        let engine = engine();
        let resumed_by = ["null", "new Promise((r) => setTimeout(r, 0))", "__TAURI__.core.invoke('x').catch(() => {})"];
        // Where the resumed code runs varies from run to run; only some places woke the hook.
        for awaited in resumed_by.iter().cycle().take(15) {
            let body = format!("await {awaited}; while (true) {{}}");
            assert_eq!(run(&engine, "p", &body).await.unwrap_err(), pause_message("p", LIMIT), "{body}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stop_outside_a_hook_is_never_blamed_on_another_plugin() {
        let engine = engine();
        let listen = "await __TAURI__.event.listen('spin', () => { while (true) {} });";
        assert!(fresh(&engine, "p", listen).await);
        assert!(!fresh(&engine, "p", listen).await);
        engine.deliver_event("p", "spin", "null");
        assert_eq!(run(&engine, "q", "throw new Error('its own');").await.unwrap_err(), "hook process error: Error: its own");
        assert!(fresh(&engine, "p", listen).await, "the stopped plugin restarts");

        let err = run_with_top(&engine, "r", "while (true) {}", "return {};").await.unwrap_err();
        assert_eq!(err, pause_message("r", LIMIT));
        assert_eq!(run(&engine, "q", "throw new Error('its own');").await.unwrap_err(), "hook process error: Error: its own");
    }

    /// Two plugins' hooks wait at once; one is stopped, and the other fails
    /// on its own just after. Each reports only what happened to it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stop_is_reported_only_by_the_hook_it_stopped() {
        let engine = engine();
        for _ in 0..10 {
            let (spin, own) = tokio::join!(
                run(&engine, "spin", "await new Promise((r) => setTimeout(r, 50)); while (true) {}"),
                run(&engine, "own", "await new Promise((r) => setTimeout(r, 100)); throw new Error('its own');"),
            );
            assert_eq!(spin.unwrap_err(), pause_message("spin", LIMIT));
            assert_eq!(own.unwrap_err(), "hook process error: Error: its own");
        }
    }

    /// A cancel stops spinning code through the same interrupt as a pause, so
    /// another plugin's stop during the cancelled hook's run must not make it
    /// report a pause. `hold` keeps the engine thread busy while the other two
    /// queue: `v` is taken in, `t` trips the limit in its own start-up, and
    /// only then does `v` spin, in its own poll, until it is cancelled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancel_is_never_reported_as_a_pause() {
        let engine = engine();
        for round in 0..5 {
            let cancel = tokio_util::sync::CancellationToken::new();
            let (h, v, t) = (format!("hold{round}"), format!("v{round}"), format!("t{round}"));
            let busy = format!("{BUSY} busy(150)");
            let hold = run_with_top(&engine, &h, &busy, "return {};");
            let victim = async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                let bundle = r#""use strict"; var P = { async process(ctx) { while (true) {} } };"#;
                let hook = engine.dispatch_hook_with_cancel(&v, bundle, "P", "process", "{}", "", "", AskedBy::Build, cancel.clone());
                tokio::time::timeout(Duration::from_secs(10), hook).await.expect("the hook must end")
            };
            let tripper = async {
                tokio::time::sleep(Duration::from_millis(60)).await;
                let r = run_with_top(&engine, &t, "while (true) {}", "return {};").await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.cancel();
                r
            };
            let (_, v_out, t_out) = tokio::join!(hold, victim, tripper);
            assert_eq!(t_out.unwrap_err(), pause_message(&t, LIMIT), "round {round}");
            assert_eq!(v_out.unwrap_err(), "hook process cancelled by host", "round {round}");
            let after = run(&engine, &format!("after{round}"), "return { ok: true };").await;
            assert!(after.as_deref().is_ok_and(|o| o.contains("true")), "round {round}: the next hook runs, got {after:?}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn only_a_refused_allocation_counts_as_out_of_memory() {
        let memory = 16 << 20;
        let (engine, _rx) = QuickJsEngine::with_budgets(None, Budgets { memory, ..Default::default() });
        assert!(fresh(&engine, "b", "").await);
        let err = run(&engine, "a", "throw new Error('out of memory');").await.unwrap_err();
        assert_eq!(err, "hook process error: Error: out of memory");
        assert!(!fresh(&engine, "b", "").await, "nothing restarted");

        let grow = "const kept = []; for (;;) kept.push(new Array(4096).fill(1));";
        assert_eq!(run(&engine, "a", grow).await.unwrap_err(), memory_message("a", memory), "a local the throw frees");
        assert!(fresh(&engine, "b", "").await, "a refusal restarts every plugin");
        run(&engine, "a", &format!("try {{ {grow} }} catch {{}} return {{}};")).await.unwrap();
        assert!(fresh(&engine, "b", "").await, "even one the plugin caught");
        run(&engine, "t", &format!("setTimeout(() => {{ {grow} }}, 0); return {{}};")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(run(&engine, "q", "throw new Error('its own');").await.unwrap_err(), "hook process error: Error: its own");
        assert!(fresh(&engine, "b", "").await, "and one in a timer");
        assert_eq!(run_with_top(&engine, "c", grow, "return {};").await.unwrap_err(), memory_message("c", memory), "at init");
        assert!(run(&engine, "c", "return { ok: true };").await.unwrap().contains("true"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn waiting_longer_than_the_limit_is_a_pause() {
        let engine = engine();
        let body = format!("{BUSY}
            for (let i = 0; i < 3; i++) {{ busy(200); await new Promise((r) => setTimeout(r, 400)); }}
            for (let i = 0; i < 3; i++) {{ busy(200); try {{ await __TAURI__.core.invoke('x'); }} catch {{}} }}
            return {{ ok: true }};");
        assert!(run(&engine, "p", &body).await.unwrap().contains("true"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plugin_past_the_memory_cap_is_stopped_and_every_plugin_restarts() {
        let memory = 16 << 20;
        let (engine, _rx) = QuickJsEngine::with_budgets(None, Budgets { memory, ..Default::default() });
        let grow = "globalThis.kept = []; for (;;) globalThis.kept.push(new Array(4096).fill(1));";
        assert!(fresh(&engine, "b", "").await);
        assert!(!fresh(&engine, "b", "").await);
        for _ in 0..2 {
            assert_eq!(run(&engine, "a", grow).await.unwrap_err(), memory_message("a", memory));
        }
        assert!(run(&engine, "a", "return { ok: true };").await.unwrap().contains("true"));
        assert!(fresh(&engine, "b", "").await, "every plugin restarts");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_timer_that_never_pauses_cannot_hold_the_engine() {
        let engine = engine();
        run(&engine, "p", "setInterval(() => { while (true) {} }, 10); return { ok: true };").await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(run(&engine, "q", "return { ok: true };").await.unwrap().contains("true"));
    }
}
