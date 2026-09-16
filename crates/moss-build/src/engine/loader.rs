//! IIFE bundle loader (report 5 §5b). Eval the esbuild IIFE, then read the plugin
//! object off globalThis by manifest global_name via a trailer assignment (strict
//! mode may not auto-attach `var X` to globalThis — the load-bearing detail).

use rquickjs::{Ctx, Object, Value, Result};

/// Eval `bundle` (a classic IIFE script, NOT a module) and return the plugin
/// instance object. Appends `;globalThis.__moss_loaded = <global_name>;` so the
/// instance is reachable regardless of strict-mode var binding.
///
/// Class-exporting plugins are NOT supported on this engine (no `new` detection) —
/// the webview path constructs them (plugin-runtime.ts:175-181); no shipped bundle
/// exports a class (D5).
pub fn load_bundle<'js>(ctx: &Ctx<'js>, bundle: &str, global_name: &str) -> Result<Object<'js>> {
    let src = format!("{bundle}\n;globalThis.__moss_loaded = {global_name};");
    ctx.eval::<(), _>(src.as_bytes())?;
    let instance: Object = ctx.globals().get("__moss_loaded")?;
    Ok(instance)
}

/// Invoke the optional `onload({ project_path })` lifecycle method on the loaded
/// instance, awaiting a returned promise. Absent onload → Ok. A throw/rejection
/// is an INIT failure (webview parity: plugin-runtime.ts:185-191; failure path
/// :208-221). onunload is deliberately NOT implemented — the webview never calls
/// it (Phase-4 plan D5). Contract is {project_path} ONLY: manifest.config rides
/// config.json via plugin-file commands (Phase-3 D4), not this call.
pub async fn call_onload<'js>(ctx: &Ctx<'js>, project_path: &str) -> std::result::Result<(), String> {
    let instance: Object = ctx.globals().get("__moss_loaded").map_err(|e| format!("{e}"))?;
    let onload_val: Value = instance.get("onload").map_err(|e| format!("{e}"))?;
    let Some(onload_fn) = onload_val.into_function() else {
        return Ok(());
    };
    let arg = Object::new(ctx.clone()).map_err(|e| format!("{e}"))?;
    arg.set("project_path", project_path).map_err(|e| format!("{e}"))?;
    let ret: Value = match onload_fn.call((arg,)) {
        Ok(v) => v,
        Err(e) => {
            if matches!(e, rquickjs::Error::Exception) {
                return Err(format!("{:?}", ctx.catch()));
            }
            return Err(format!("{e}"));
        }
    };
    let mp = rquickjs::promise::MaybePromise::from_value(ret);
    // Turbofish keeps T inference robust (review minor 2026-06-11): a bare
    // `into_future()` + struct-pattern Ok arm leaves T to flow backwards from
    // the pattern — fragile. Pin it explicitly and discard the value.
    match mp.into_future::<Value>().await {
        Ok(_) => Ok(()),
        Err(e) => {
            if matches!(e, rquickjs::Error::Exception) {
                return Err(format!("{:?}", ctx.catch()));
            }
            Err(format!("{e}"))
        }
    }
}

/// Set window/globalThis.__MOSS_INTERNAL_CONTEXT__ before a hook; clear after.
pub fn set_internal_context(
    ctx: &Ctx<'_>,
    plugin_name: &str,
    project_path: &str,
    moss_dir: &str,
) -> Result<()> {
    let obj = Object::new(ctx.clone())?;
    obj.set("plugin_name", plugin_name)?;
    obj.set("project_path", project_path)?;
    obj.set("moss_dir", moss_dir)?;
    ctx.globals().set("__MOSS_INTERNAL_CONTEXT__", obj)?;
    Ok(())
}

pub fn clear_internal_context(ctx: &Ctx<'_>) -> Result<()> {
    // Set the global back to `undefined`. NOTE: bare `rquickjs::Undefined` is a
    // '_js-bearing handle TYPE, not a unit value — `set("..", rquickjs::Undefined)`
    // does NOT compile. The robust, zero-fuss primary form is a one-line eval:
    ctx.eval::<(), _>("globalThis.__MOSS_INTERNAL_CONTEXT__ = undefined")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn loads_strict_iife_and_reads_global() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        // Mimic an esbuild strict-mode IIFE assigned to a top-level var.
        let bundle = r#""use strict"; var TestPlugin = (() => ({ async process(ctx) { return ctx.n + 1; } }))();"#;
        let got: i64 = async_with!(ctx => |ctx| {
            let instance = load_bundle(&ctx, bundle, "TestPlugin").unwrap();
            ctx.globals().set("__inst", instance).unwrap();
            // call process({n:41}) — returns a promise; resolve via into_future.
            let p: rquickjs::Promise = ctx.eval("__inst.process({ n: 41 })").unwrap();
            p.into_future::<i64>().await.unwrap()
        }).await;
        assert_eq!(got, 42);
    }
}
