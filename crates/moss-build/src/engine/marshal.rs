//! serde_json::Value ⇆ rquickjs::Value marshaling.
//!
//! rquickjs 0.12 has NO serde feature (research report 1 §3). The verified
//! bridge is Ctx::json_parse / json_stringify (ctx.rs:280 / :301) — go through
//! a JSON string. `undefined`/functions/symbols drop (JSON.stringify semantics);
//! NaN/Infinity become null. This matches the WKWebview __TAURI__.invoke behavior,
//! so it is behavior-preserving for the shim.

use rquickjs::{Ctx, Error, Result, Value};
use serde_json::Value as Json;

/// serde_json::Value -> rquickjs Value (inside a Ctx).
pub fn json_to_js<'js>(ctx: &Ctx<'js>, j: &Json) -> Result<Value<'js>> {
    let s = serde_json::to_string(j).map_err(|_| Error::Unknown)?;
    ctx.json_parse(s)
}

/// rquickjs Value -> serde_json::Value (None JS value `undefined` => Json::Null).
pub fn js_to_json<'js>(ctx: &Ctx<'js>, v: Value<'js>) -> Result<Json> {
    match ctx.json_stringify(v)? {
        Some(s) => {
            let s = s.to_string()?;
            serde_json::from_str(&s).map_err(|_| Error::Unknown)
        }
        None => Ok(Json::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};
    use serde_json::json;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trips_nested_json_through_js() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let input = json!({ "name": "x", "n": 7, "nested": { "arr": [1, 2, 3], "b": true } });
        // Convert IN, read a field back OUT — never let a Value<'js> escape the closure.
        let out: Json = async_with!(ctx => |ctx| {
            let v = json_to_js(&ctx, &input).unwrap();
            ctx.globals().set("__probe", v).unwrap();
            let back: Value = ctx.eval("__probe").unwrap();
            js_to_json(&ctx, back).unwrap()
        })
        .await;
        assert_eq!(out, input);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn undefined_becomes_null() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let out: Json = async_with!(ctx => |ctx| {
            let v: Value = ctx.eval("undefined").unwrap();
            js_to_json(&ctx, v).unwrap()
        })
        .await;
        assert_eq!(out, Json::Null);
    }
}
