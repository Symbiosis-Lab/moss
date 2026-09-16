//! globalThis.__TAURI__ assembly + the engine-side event hub (report 5 §3).
//! __TAURI__ = { core: { invoke }, event: { emit, listen } }.
//!
//! LOAD-BEARING INVARIANT for every async host fn (`Func::from(Async(closure))`):
//! the closure body UP TO `async move {` runs WHILE THE RUNTIME LOCK IS HELD — so that
//! is the ONLY place a Value/Object/Function<'js> may be touched (json_stringify,
//! obj.get, Persistent::save, Function::new, hub.borrow_mut). The spawned async body
//! runs OUT of lock and holds ONLY Send owned data + host I/O — NEVER a JS handle.

use rquickjs::{
    function::{Async, Opt},
    prelude::Func,
    Ctx, Function, Object, Result,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::app_host::SharedAppHost;

/// Per-Context registry of JS listeners: event name -> [(id, handler)], stored as
/// Persistent<Function> (runtime-rooted, '_js-FREE) so the hub outlives the listen
/// job's async_with! closure and is restored in the later deliver job. NO 'js param.
#[derive(Default)]
pub struct EventHub {
    /// `pub(crate)` so engine teardown can `.clear()` these `Persistent` handles
    /// WHILE THE RUNTIME IS STILL ALIVE — `Persistent::drop` unroots from the live
    /// runtime, so clearing here (before the Context drops) keeps the listener JS
    /// objects out of `gc_obj_list` at `JS_FreeRuntime` (avoids the quickjs-ng abort).
    /// (id, handler) pairs per event name. Ids let unlisten remove BY IDENTITY.
    pub(crate) listeners: HashMap<String, Vec<(u64, rquickjs::Persistent<rquickjs::Function<'static>>)>>,
    next_id: u64,
}

impl EventHub {
    /// Register a listener; returns its id for unlisten.
    pub(crate) fn add(&mut self, name: String, f: rquickjs::Persistent<rquickjs::Function<'static>>) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.listeners.entry(name).or_default().push((id, f));
        id
    }
    /// Remove one listener by identity. Dropping the Persistent here is safe:
    /// unlisten runs on the engine thread while the runtime is alive.
    pub(crate) fn remove(&mut self, name: &str, id: u64) {
        if let Some(v) = self.listeners.get_mut(name) {
            v.retain(|(lid, _)| *lid != id);
        }
    }
}

/// Get-or-create globalThis.__TAURI__ and set a member on it.
pub fn set_tauri_member<'js>(ctx: &Ctx<'js>, key: &str, value: Object<'js>) -> Result<()> {
    let globals = ctx.globals();
    let tauri: Object = match globals.get("__TAURI__") {
        Ok(obj) => obj,
        Err(_) => {
            let o = Object::new(ctx.clone())?;
            globals.set("__TAURI__", o.clone())?;
            o
        }
    };
    tauri.set(key, value)?;
    Ok(())
}

/// Build the unlisten function: removes (name, id) from the hub. The closure
/// captures Rc handles — fine, it only ever runs on the engine thread.
///
/// VERIFY-ON-BUILD: `Function::new` with a `!Send` `Rc` capture — rquickjs
/// (non-`parallel`) accepts non-Send closures here; if the bound complains,
/// the fallback is `Func::from(move || ..)` set on a fresh `Function` via
/// `ctx.eval`-free path (same shape as existing sync shims in `web_shims.rs`).
fn make_unlisten<'js>(
    ctx: Ctx<'js>,
    hub: Rc<RefCell<EventHub>>,
    name: String,
    id: u64,
) -> Result<Function<'js>> {
    Function::new(ctx, move || {
        hub.borrow_mut().remove(&name, id);
    })
}

/// Install __TAURI__.event = { emit, listen }. `app` is Option so the Task-14
/// acceptance gate (no Tauri app) can pass None — emit becomes a no-op then
/// (except `plugin-message`, which rides the bridge regardless of app).
///
/// Task 11a: `bridge` + `plugin` wire the two host hops —
/// - emit("plugin-message", ..) routes over the bridge as a `plugin_message`
///   dispatch (the keyed sink map intercepts it — Task 1);
/// - listen(name, ..) notifies the host (`__engine_listen__`) so it registers
///   ONE forwarding `app.listen(name)` into `engine.deliver_event` (Task 11b).
pub fn install_event<'js>(
    ctx: &Ctx<'js>,
    app: Option<SharedAppHost>,
    hub: Rc<RefCell<EventHub>>,
    bridge: super::host_fns::DispatchBridge,
    plugin: &str,
) -> Result<()> {
    let event = Object::new(ctx.clone())?;

    // emit(name, payload?) -> Promise<void>. The closure returns `()`, so it
    // compiles directly (no free-fn forward needed). SYNCHRONOUS PRELUDE (lock
    // held): stringify the payload HERE; the async body holds only owned
    // Strings + the bridge + the shared app host (the bridge call is the only
    // await; Send owned data only).
    // The `ctx` and `payload` params share a single `'js` so `json_stringify(v)`
    // (which ties the Value's lifetime to ctx's) type-checks — distinct `'_`s make
    // `Value<'js>` invariance reject it.
    let app_emit = app.clone();
    let bridge_emit = bridge.clone();
    event.set(
        "emit",
        Func::from(Async(
            move |ctx: Ctx<'js>, name: String, payload: Opt<rquickjs::Value<'js>>| {
                let app = app_emit.clone();
                let bridge = bridge_emit.clone();
                // SYNCHRONOUS PRELUDE (lock held, JS access OK): stringify the payload.
                let json = payload
                    .0
                    .and_then(|v| ctx.json_stringify(v).ok().flatten())
                    .and_then(|s| s.to_string().ok());
                async move {
                    if name == "plugin-message" {
                        // Progress/log heartbeats MUST reach the keyed sink map →
                        // route_signal → PluginHookState activity stamps, or quickjs
                        // hooks die at 60s inactivity despite heartbeats (probe R4).
                        // Payload {pluginName, hookName, message} == the plugin_message
                        // invoke args — reuse the Task-1 interception verbatim.
                        let _ = bridge.call("plugin_message".to_string(),
                            json.unwrap_or_else(|| "null".to_string())).await;
                    } else if let Some(app) = app {
                        // Everything else is the app's routing decision — toasts to
                        // the typed MossEvent bus, broadcast names to every webview,
                        // custom events to the browser panel. Headless there is
                        // nowhere to paint any of it, so emit is a no-op.
                        app.emit_plugin_event(&name, json);
                    }
                    Ok::<(), rquickjs::Error>(())
                }
            },
        )),
    )?;

    // listen(name, handler) -> Promise<unlistenFn>. Register handler in the hub.
    // SYNCHRONOUS PRELUDE (lock held, JS access OK): save the Persistent + record it,
    // then build the unlisten fn. The future returns `Function<'js>`, which a closure
    // cannot generalize over the invariant `'js` — so the unlisten is constructed in
    // the prelude (via the free fn `make_unlisten`) and the async body fires the
    // host-side registration (owned Strings + bridge only) before returning it.
    let hub_listen = hub.clone();
    let bridge_listen = bridge.clone();
    let plugin_listen = plugin.to_string();
    event.set(
        "listen",
        Func::from(Async(
            move |ctx: Ctx<'js>, name: String, handler: Function<'js>| {
                let hub = hub_listen.clone();
                let bridge = bridge_listen.clone();
                let plugin = plugin_listen.clone();
                let persistent = rquickjs::Persistent::save(&ctx, handler);
                let id = hub.borrow_mut().add(name.clone(), persistent);
                let host_name = name.clone();
                let unlisten = make_unlisten(ctx, hub, name, id);
                async move {
                    // Host-side bridge registration: ONE app.listen per (plugin, name),
                    // forwarding the Tauri bus into engine.deliver_event (Task 11b).
                    let _ = bridge.call("__engine_listen__".to_string(),
                        serde_json::json!({ "plugin": plugin, "name": host_name }).to_string()).await;
                    unlisten
                }
            },
        )),
    )?;

    set_tauri_member(ctx, "event", event)?;
    Ok(())
}

/// Host -> engine event delivery: invoke every registered listener for `name`
/// with `{ payload }`. Called inside an async_with! closure on the engine thread.
//
// Consumed by the host-side delivery path (engine `Job::DeliverEvent`) + the
// acceptance gate.
pub fn deliver(ctx: &Ctx<'_>, hub: &Rc<RefCell<EventHub>>, name: &str, payload_json: &str) -> Result<()> {
    // Snapshot the callbacks for `name` and DROP the borrow before invoking any —
    // a handler may call back into listen()/unlisten and re-borrow the hub.
    // `restore` CONSUMES the Persistent, so clone the stored ones (Function<'static>
    // is Clone) to leave the registry intact for future deliveries.
    let cbs = hub
        .borrow()
        .listeners
        .get(name)
        .cloned()
        .unwrap_or_default();
    for (_, cb) in cbs {
        let one = (|| -> Result<()> {
            let f: Function = cb.restore(ctx)?;
            let payload: rquickjs::Value = ctx.json_parse(payload_json.to_string())?;
            let evt = Object::new(ctx.clone())?;
            evt.set("payload", payload)?;
            f.call::<_, ()>((evt,))?;
            Ok(())
        })();
        if let Err(e) = one {
            // Clear any pending exception so the NEXT listener runs clean, then log.
            let _ = ctx.catch();
            log::warn!(target: "plugin", "event '{name}' listener failed: {e}");
        }
    }
    Ok(())
}

// The emit-side helpers — `emit_to_panel`, `plugin_toast_payload`, the
// `parse_plugin_show_toast*` pair and `emit_payload_value` — live in
// `plugins::tauri_app_host` with the app half of the seam. The default emit
// routing they implement is documented on `AppHost::emit_plugin_event`.

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn listen_then_deliver_fires_handler() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let rt = AsyncRuntime::new().unwrap();
                let ctx = AsyncContext::full(&rt).await.unwrap();
                async_with!(ctx => |ctx| {
                    let hub: Rc<RefCell<EventHub>> = Rc::new(RefCell::new(EventHub::default()));
                    ctx.globals().set("__seen", false).unwrap();
                    let handler: Function = ctx.eval("(e) => { globalThis.__seen = e.payload.reason === 'user'; }").unwrap();
                    hub.borrow_mut().add("browser-closed".into(), rquickjs::Persistent::save(&ctx, handler));
                    deliver(&ctx, &hub, "browser-closed", r#"{"reason":"user"}"#).unwrap();
                    let seen: bool = ctx.eval("globalThis.__seen").unwrap();
                    assert!(seen);
                }).await;
            })
            .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unlisten_removes_the_listener() {
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            async_with!(ctx => |ctx| {
                let hub: Rc<RefCell<EventHub>> = Rc::new(RefCell::new(EventHub::default()));
                ctx.globals().set("__hits", 0).unwrap();
                let handler: Function = ctx.eval("() => { globalThis.__hits += 1; }").unwrap();
                let id = hub.borrow_mut().add("browser-closed".into(), rquickjs::Persistent::save(&ctx, handler));
                deliver(&ctx, &hub, "browser-closed", "null").unwrap();
                hub.borrow_mut().remove("browser-closed", id);
                deliver(&ctx, &hub, "browser-closed", "null").unwrap();
                let hits: i32 = ctx.eval("globalThis.__hits").unwrap();
                assert_eq!(hits, 1, "second deliver must not reach the removed listener");
            }).await;
        }).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deliver_isolates_a_throwing_listener() {
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            async_with!(ctx => |ctx| {
                let hub: Rc<RefCell<EventHub>> = Rc::new(RefCell::new(EventHub::default()));
                ctx.globals().set("__second", false).unwrap();
                let bad: Function = ctx.eval("() => { throw new Error('listener boom'); }").unwrap();
                let good: Function = ctx.eval("() => { globalThis.__second = true; }").unwrap();
                hub.borrow_mut().add("e".into(), rquickjs::Persistent::save(&ctx, bad));
                hub.borrow_mut().add("e".into(), rquickjs::Persistent::save(&ctx, good));
                // Browser semantics: a throwing handler must not stop later handlers,
                // and deliver itself must not return Err for a handler throw.
                deliver(&ctx, &hub, "e", "null").unwrap();
                let second: bool = ctx.eval("globalThis.__second").unwrap();
                assert!(second, "the second listener must fire despite the first throwing");
            }).await;
        }).await;
    }
}
