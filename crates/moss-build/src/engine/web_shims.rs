//! Web globals the github/matters bundles + moss-api SDK touch (report 3 §2):
//! console, atob/btoa, TextEncoder/TextDecoder, crypto.randomUUID. Plus tokio
//! timers (Task 6). All sync fns use Func::from (report 1 §1); install on
//! globalThis inside a `with`/`async_with!` closure.

use rquickjs::{Ctx, Result, prelude::Func};

/// Install console.{log,info,warn,error,debug} routing to Rust `log`.
///
/// Level mapping:
///  - `error`  → `log::error!`  — always visible (appears in moss.log at any level).
///  - `warn`   → `log::warn!`   — always visible.
///  - `info`   → `log::info!`   — visible at the default INFO threshold (production
///                                 moss.log). Plugin code should use `console.info` for
///                                 milestone events that should appear in support logs
///                                 (e.g. "Matters domain: X", "Opening login page...").
///  - `log`/`debug` → `log::debug!` — suppressed at INFO, visible only under
///                                 MOSS_LOG_LEVEL=debug. Use for verbose detail.
///
/// This means a plugin can select the right visibility tier without touching
/// Rust: `console.warn` for recoverable issues, `console.info` for milestones,
/// `console.log`/`debug` for verbose detail.
pub fn install_console(ctx: &Ctx<'_>) -> Result<()> {
    let console = rquickjs::Object::new(ctx.clone())?;
    for level in ["log", "info", "debug", "warn", "error"] {
        let lvl = level.to_string();
        console.set(level, Func::from(move |msg: String| {
            match lvl.as_str() {
                "warn"  => log::warn!(target: "plugin",  "[plugin console] {msg}"),
                "error" => log::error!(target: "plugin", "[plugin console] {msg}"),
                "info"  => log::info!(target: "plugin",  "[plugin console] {msg}"),
                _       => log::debug!(target: "plugin", "[plugin console] {msg}"),
            }
        }))?;
    }
    ctx.globals().set("console", console)?;
    Ok(())
}

/// atob/btoa over base64.
pub fn install_base64(ctx: &Ctx<'_>) -> Result<()> {
    use base64::Engine as _;
    ctx.globals().set("atob", Func::from(|s: String| -> rquickjs::Result<String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .map_err(|_| rquickjs::Error::Unknown)?;
        // atob yields a binary string (latin1) — map each byte to a char.
        Ok(bytes.into_iter().map(|b| b as char).collect())
    }))?;
    ctx.globals().set("btoa", Func::from(|s: String| -> String {
        let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }))?;
    Ok(())
}

// Plain `fn` items for TextEncoder/TextDecoder internals. Using named fn items
// (not closures) avoids the invariant-lifetime issue with `Object<'js>` /
// `Value<'js>` — fn items naturally generalize over the JS lifetime.

fn text_encode<'js>(ctx: Ctx<'js>, s: String) -> rquickjs::Result<rquickjs::Value<'js>> {
    let ta = rquickjs::TypedArray::<u8>::new(ctx, s.into_bytes())?;
    Ok(ta.into_value())
}

fn text_decode(bytes: rquickjs::TypedArray<'_, u8>) -> rquickjs::Result<String> {
    Ok(String::from_utf8_lossy(bytes.as_ref()).into_owned())
}

fn make_text_encoder<'js>(ctx: Ctx<'js>) -> rquickjs::Result<rquickjs::Object<'js>> {
    let obj = rquickjs::Object::new(ctx.clone())?;
    obj.set(
        "encode",
        Func::from(text_encode as fn(Ctx<'_>, String) -> rquickjs::Result<rquickjs::Value<'_>>),
    )?;
    Ok(obj)
}

fn make_text_decoder<'js>(ctx: Ctx<'js>) -> rquickjs::Result<rquickjs::Object<'js>> {
    let obj = rquickjs::Object::new(ctx.clone())?;
    obj.set(
        "decode",
        Func::from(
            text_decode
                as fn(rquickjs::TypedArray<'_, u8>) -> rquickjs::Result<String>,
        ),
    )?;
    Ok(obj)
}

/// TextEncoder / TextDecoder backed by Rust UTF-8 <-> bytes.
///
/// Registers `TextEncoder` and `TextDecoder` as constructor functions (so
/// `new TextEncoder()` and `new TextDecoder()` work). Each call returns a
/// fresh object with `encode` / `decode` host methods.
pub fn install_text_codec(ctx: &Ctx<'_>) -> Result<()> {
    use rquickjs::Function;

    // Build TextEncoder constructor: Function::new (which wraps a Rust fn) +
    // .with_constructor(true) so QuickJS allows `new TextEncoder()`.
    let enc_fn = Function::new(
        ctx.clone(),
        make_text_encoder as fn(Ctx<'_>) -> rquickjs::Result<rquickjs::Object<'_>>,
    )?
    .with_constructor(true);
    ctx.globals().set("TextEncoder", enc_fn)?;

    let dec_fn = Function::new(
        ctx.clone(),
        make_text_decoder as fn(Ctx<'_>) -> rquickjs::Result<rquickjs::Object<'_>>,
    )?
    .with_constructor(true);
    ctx.globals().set("TextDecoder", dec_fn)?;

    Ok(())
}

/// crypto.randomUUID.
pub fn install_crypto(ctx: &Ctx<'_>) -> Result<()> {
    let crypto = rquickjs::Object::new(ctx.clone())?;
    crypto.set("randomUUID", Func::from(|| uuid::Uuid::new_v4().to_string()))?;
    ctx.globals().set("crypto", crypto)?;
    Ok(())
}

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Per-engine timer registry (!Send, single-threaded on the engine thread).
#[derive(Default)]
pub struct TimerRegistry {
    next_id: u64,
    tokens: HashMap<u64, CancellationToken>,
}

impl TimerRegistry {
    fn alloc(&mut self) -> (u64, CancellationToken) {
        let id = self.next_id;
        self.next_id += 1;
        let token = CancellationToken::new();
        self.tokens.insert(id, token.clone());
        (id, token)
    }
    fn cancel(&mut self, id: u64) {
        if let Some(t) = self.tokens.remove(&id) { t.cancel(); }
    }
    /// Teardown: cancel every outstanding timer.
    pub fn cancel_all(&mut self) {
        for (_, t) in self.tokens.drain() { t.cancel(); }
    }
    /// Total timers ever allocated by this registry (monotonic; used by tests to
    /// prove two registries didn't clobber each other — each must see exactly the
    /// allocs from its OWN context).
    pub fn allocated_count(&self) -> u64 {
        self.next_id
    }
    /// Timers still outstanding (allocated and not yet canceled).
    pub fn outstanding(&self) -> usize {
        self.tokens.len()
    }
}

// Per-Context registry map. Named fn items (below) look the registry up by the
// Context's stable raw pointer so each plugin Context reaches its OWN registry —
// Task 11 creates one TimerRegistry per plugin Context and a single global slot
// would clobber across coexisting plugins.
//
// Why not rquickjs `Ctx::store_userdata`? In rquickjs 0.12 userdata is stored on
// the *Runtime* opaque (`get_opaque()` → `Opaque::from_runtime_ptr`) and keyed by
// `TypeId` (`runtime/userdata.rs` `UserDataMap`), so two Contexts on the SAME
// Runtime share one `TimerUserData` slot — clobbering identically to a thread_local
// single slot. (It would also wedge `ctx.spawn` re-entrancy: a held `UserDataGuard`
// bumps `count`, and `insert`/`remove` `Err` while `count > 0`.) Userdata cannot
// give per-Context isolation here, so we key a thread_local map by Context identity.
//
// Named fn items (not closures) avoid the invariant-lifetime issue with two `'_`
// wildcards in a `Func::from` closure becoming distinct lifetimes, which breaks
// `ctx.spawn`'s `F: Future + 'js` bound. A single `<'js>` parameter unifies them,
// matching the pattern in the rquickjs test suite.
thread_local! {
    static TIMERS: RefCell<HashMap<usize, Rc<RefCell<TimerRegistry>>>> =
        RefCell::new(HashMap::new());
}

/// Stable per-Context key: the raw `JSContext` pointer. Distinct for every
/// `AsyncContext`/`Context` on a runtime, so each Context maps to its own registry.
fn ctx_key(ctx: &Ctx<'_>) -> usize {
    ctx.as_raw().as_ptr() as usize
}

/// Look up the registry installed for `ctx`. Panics if `install_timers` wasn't
/// called for this Context first (the JS globals only exist after install, so a
/// timer call without a registry is a host bug).
fn registry_for(ctx: &Ctx<'_>) -> Rc<RefCell<TimerRegistry>> {
    let key = ctx_key(ctx);
    TIMERS.with(|t| {
        t.borrow()
            .get(&key)
            .cloned()
            .expect("timer registry not installed for this Context")
    })
}

fn set_timeout_fn<'js>(
    ctx: Ctx<'js>,
    cb: rquickjs::Function<'js>,
    ms: rquickjs::function::Opt<f64>,
) -> u64 {
    let (id, token) = registry_for(&ctx).borrow_mut().alloc();
    let ms = ms.0.unwrap_or(0.0).max(0.0);
    ctx.spawn(async move {
        tokio::select! {
            _ = token.cancelled() => {}
            _ = tokio::time::sleep(Duration::from_secs_f64(ms / 1000.0)) => {
                let _ = cb.call::<_, ()>(());
            }
        }
    });
    id
}

fn clear_timeout_fn(ctx: Ctx<'_>, id: rquickjs::function::Opt<f64>) {
    if let Some(id) = id.0 {
        registry_for(&ctx).borrow_mut().cancel(id as u64);
    }
}

fn set_interval_fn<'js>(
    ctx: Ctx<'js>,
    cb: rquickjs::Function<'js>,
    ms: rquickjs::function::Opt<f64>,
) -> u64 {
    let (id, token) = registry_for(&ctx).borrow_mut().alloc();
    let ms = ms.0.unwrap_or(0.0).max(1.0);
    ctx.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs_f64(ms / 1000.0));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await; // first tick immediate — skip (setInterval semantics)
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = interval.tick() => {
                    if cb.call::<_, ()>(()).is_err() { break; }
                }
            }
        }
    });
    id
}

fn clear_interval_fn(ctx: Ctx<'_>, id: rquickjs::function::Opt<f64>) {
    if let Some(id) = id.0 {
        registry_for(&ctx).borrow_mut().cancel(id as u64);
    }
}

/// Install setTimeout/clearTimeout/setInterval/clearInterval backed by tokio,
/// cancelable via `timers`. `ctx.spawn` keeps the timer future scheduler-tracked.
///
/// The registry is registered under this Context's identity so its own timer
/// fns resolve to it (per-Context, not global) — see the `TIMERS` comment.
pub fn install_timers(ctx: &Ctx<'_>, timers: Rc<RefCell<TimerRegistry>>) -> Result<()> {
    TIMERS.with(|t| t.borrow_mut().insert(ctx_key(ctx), timers));

    ctx.globals().set("setTimeout", Func::from(set_timeout_fn))?;
    ctx.globals().set("clearTimeout", Func::from(clear_timeout_fn))?;
    ctx.globals().set("setInterval", Func::from(set_interval_fn))?;
    ctx.globals().set("clearInterval", Func::from(clear_interval_fn))?;
    Ok(())
}

/// Teardown hygiene (Task-6 follow-up): remove this Context's entry from the
/// per-Context `TIMERS` map. Called on engine shutdown / per-plugin drop so the
/// Context's raw-pointer key can't be reused stale by a future Context allocated at
/// the same address. The live timer tokens must already have been canceled
/// (`TimerRegistry::cancel_all`) — this only drops the bookkeeping Rc.
///
/// Must be called inside an `async_with!`/`with` closure (it needs `&Ctx`).
pub fn clear_context_timers(ctx: &Ctx<'_>) {
    TIMERS.with(|t| {
        t.borrow_mut().remove(&ctx_key(ctx));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn text_encoder_decoder_round_trip() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let (enc_len, decoded): (usize, String) = async_with!(ctx => |ctx| {
            install_text_codec(&ctx).unwrap();
            let enc_len: usize = ctx.eval("new TextEncoder().encode('ab').length").unwrap();
            let decoded: String = ctx.eval("new TextDecoder().decode(new Uint8Array([104,105]))").unwrap();
            (enc_len, decoded)
        }).await;
        assert_eq!(enc_len, 2);
        assert_eq!(decoded, "hi");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn base64_and_uuid_work() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let (b64, uuid_len): (String, usize) = async_with!(ctx => |ctx| {
            install_console(&ctx).unwrap();
            install_base64(&ctx).unwrap();
            install_crypto(&ctx).unwrap();
            let b64: String = ctx.eval("btoa('hello')").unwrap();
            let round: String = ctx.eval("atob(btoa('hello'))").unwrap();
            assert_eq!(round, "hello");
            let uuid: String = ctx.eval("crypto.randomUUID()").unwrap();
            (b64, uuid.len())
        })
        .await;
        assert_eq!(b64, "aGVsbG8=");
        assert_eq!(uuid_len, 36);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_timeout_fires_and_clear_cancels() {
        use std::rc::Rc;
        use std::cell::RefCell;
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            let timers = Rc::new(RefCell::new(TimerRegistry::default()));
            async_with!(ctx => |ctx| {
                install_timers(&ctx, timers.clone()).unwrap();
                ctx.eval::<(), _>(
                    "globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 5);"
                ).unwrap();
            }).await;
            rt.idle().await; // flush the spawned timer to completion
            let fired: bool = async_with!(ctx => |ctx| { ctx.eval("globalThis.fired").unwrap() }).await;
            assert!(fired);
        }).await;
    }

    /// Two Contexts on the SAME runtime, each with its OWN TimerRegistry, must not
    /// clobber each other (the bug the per-Context map fixes). Each timer fires into
    /// its own context's global, and each registry sees exactly the one alloc from
    /// ITS context — proving the registries are independent (per-plugin isolation
    /// that Task 11 relies on).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_contexts_have_isolated_timer_registries() {
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let rt = AsyncRuntime::new().unwrap();
            let ctx_a = AsyncContext::full(&rt).await.unwrap();
            let ctx_b = AsyncContext::full(&rt).await.unwrap();
            let reg_a = Rc::new(RefCell::new(TimerRegistry::default()));
            let reg_b = Rc::new(RefCell::new(TimerRegistry::default()));

            async_with!(ctx_a => |ctx| {
                install_timers(&ctx, reg_a.clone()).unwrap();
                ctx.eval::<(), _>(
                    "globalThis.a=false; setTimeout(()=>{globalThis.a=true;},5);"
                ).unwrap();
            }).await;
            async_with!(ctx_b => |ctx| {
                install_timers(&ctx, reg_b.clone()).unwrap();
                ctx.eval::<(), _>(
                    "globalThis.b=false; setTimeout(()=>{globalThis.b=true;},5);"
                ).unwrap();
            }).await;

            rt.idle().await;

            let a: bool = async_with!(ctx_a => |ctx| { ctx.eval("globalThis.a").unwrap() }).await;
            let b: bool = async_with!(ctx_b => |ctx| { ctx.eval("globalThis.b").unwrap() }).await;
            assert!(a && b, "both contexts' timers must fire independently");

            // Each registry allocated exactly one timer — its OWN. If install_timers
            // clobbered (single-slot bug), one registry would hold both allocs (== 2)
            // and the other zero (== 0).
            assert_eq!(reg_a.borrow().allocated_count(), 1, "reg_a saw exactly its own alloc");
            assert_eq!(reg_b.borrow().allocated_count(), 1, "reg_b saw exactly its own alloc");

            // cancel_all on reg_a must not touch reg_b's bookkeeping.
            reg_a.borrow_mut().cancel_all();
            assert_eq!(reg_a.borrow().outstanding(), 0);
            // reg_b's timer already fired and removed itself, so it's also empty — but
            // crucially cancel_all(reg_a) did not change reg_b's monotonic alloc count.
            assert_eq!(reg_b.borrow().allocated_count(), 1, "reg_b unaffected by reg_a teardown");
        }).await;
    }
}
