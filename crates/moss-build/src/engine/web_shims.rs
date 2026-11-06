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

/// crypto.randomUUID, and the part of `crypto.subtle` plugins sign requests
/// with: `importKey("raw", …)` of an HMAC key and `sign("HMAC", …)`, shaped as
/// the Web Crypto API is in browsers, Node and Deno, so the same plugin code
/// runs in moss and under a test runner.
pub fn install_crypto(ctx: &Ctx<'_>) -> Result<()> {
    let crypto = rquickjs::Object::new(ctx.clone())?;
    crypto.set("randomUUID", Func::from(|| uuid::Uuid::new_v4().to_string()))?;
    ctx.globals().set("crypto", crypto)?;
    let install: rquickjs::Function = ctx.eval(SUBTLE_JS)?;
    let hashes: Vec<&str> = HMAC_HASHES.iter().map(|(name, _)| *name).collect();
    install.call::<_, ()>((
        rquickjs::Function::new(ctx.clone(), hmac_sign as for<'js> fn(Ctx<'js>, String, rquickjs::TypedArray<'js, u8>, rquickjs::TypedArray<'js, u8>) -> Result<rquickjs::Value<'js>>)?,
        hashes,
    ))?;
    Ok(())
}

const HMAC_HASHES: [(&str, ring::hmac::Algorithm); 4] = [
    ("SHA-1", ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
    ("SHA-256", ring::hmac::HMAC_SHA256),
    ("SHA-384", ring::hmac::HMAC_SHA384),
    ("SHA-512", ring::hmac::HMAC_SHA512),
];

fn hmac_sign<'js>(ctx: Ctx<'js>, hash: String, key: rquickjs::TypedArray<'js, u8>, data: rquickjs::TypedArray<'js, u8>) -> Result<rquickjs::Value<'js>> {
    let Some((_, algorithm)) = HMAC_HASHES.iter().find(|(name, _)| *name == hash) else {
        return Err(rquickjs::Exception::throw_type(&ctx, &format!("unsupported hash {hash}")));
    };
    let tag = ring::hmac::sign(&ring::hmac::Key::new(*algorithm, key.as_ref()), data.as_ref());
    Ok(rquickjs::TypedArray::<u8>::new(ctx, tag.as_ref().to_vec())?.into_value())
}

/// Wraps the host's HMAC in the Web Crypto shapes. What a key may do and its
/// bytes are kept in a closure, so a key object shows only its type,
/// algorithm and usages, and changing those cannot change what it signs.
/// Errors carry the Web Crypto names on a plain Error, as the runtime has no
/// DOMException.
const SUBTLE_JS: &str = r#"(hmac, HASHES) => {
  const keys = new WeakMap();
  const nameOf = (a) => String(typeof a === "string" ? a : a?.name ?? "").toUpperCase();
  const error = (name, message) => Object.assign(new Error(message), { name });
  const bytes = (data) => {
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw new TypeError("expected an ArrayBuffer or a view of one");
  };
  class CryptoKey {
    constructor() { throw new TypeError("Illegal constructor"); }
  }
  const subtle = {
    async importKey(format, keyData, algorithm, extractable, usages) {
      if (format !== "raw" || nameOf(algorithm) !== "HMAC") throw error("NotSupportedError", "moss's crypto.subtle imports raw HMAC keys only");
      if (algorithm.hash === undefined) throw new TypeError("an HMAC key needs a hash");
      const hash = HASHES.find((h) => h === nameOf(algorithm.hash));
      if (!hash) throw error("NotSupportedError", `unsupported hash ${nameOf(algorithm.hash)}`);
      if (!Array.isArray(usages) || !usages.length || usages.some((u) => u !== "sign" && u !== "verify")) throw error("SyntaxError", "an HMAC key's usages are sign and verify");
      const raw = bytes(keyData).slice();
      if (!raw.length) throw error("DataError", "an HMAC key cannot be empty");
      let length = raw.length * 8;
      if (algorithm.length !== undefined) {
        // The standard lets a key name a length within its last byte.
        if (!(algorithm.length > length - 8 && algorithm.length <= length)) throw error("DataError", `length ${algorithm.length} does not fit a ${length}-bit key`);
        length = algorithm.length;
      }
      const value = (v) => ({ value: v, enumerable: true });
      const key = Object.create(CryptoKey.prototype, {
        type: value("secret"),
        extractable: value(Boolean(extractable)),
        algorithm: value({ name: "HMAC", hash: { name: hash }, length }),
        usages: value([...usages]),
      });
      keys.set(key, { raw, hash, usages: [...usages] });
      return key;
    },
    async sign(algorithm, key, data) {
      const held = keys.get(key);
      if (nameOf(algorithm) !== "HMAC" || !held || !held.usages.includes("sign")) throw error("InvalidAccessError", "the key is not an HMAC key that may sign");
      return hmac(held.hash, held.raw, bytes(data)).buffer;
    },
  };
  Object.defineProperty(globalThis.crypto, "subtle", { value: subtle, enumerable: true });
  globalThis.CryptoKey = CryptoKey;
}"#;

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

    /// Runs `code` (top-level await allowed) with `crypto` installed and returns its completion value.
    async fn with_crypto(code: &'static str) -> String {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        async_with!(ctx => |ctx| {
            install_crypto(&ctx).unwrap();
            install_text_codec(&ctx).unwrap();
            let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
            let envelope: rquickjs::Object = promise.into_future().await.unwrap();
            envelope.get::<_, String>("value").unwrap()
        })
        .await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subtle_signs_hmac_as_the_rfc_test_vectors_do() {
        // RFC 2202 and RFC 4231, test case 2 of each.
        let tags = with_crypto(r#"
            const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");
            const enc = new TextEncoder();
            // Key and data arrive as views into larger buffers, and the key's buffer is overwritten after import.
            const tag = async (hash) => {
                const keyBytes = enc.encode("..Jefe");
                const key = await crypto.subtle.importKey("raw", keyBytes.subarray(2), { name: "HMAC", hash }, false, ["sign"]);
                keyBytes.fill(0);
                return hex(await crypto.subtle.sign("HMAC", key, enc.encode("..what do ya want for nothing?").subarray(2)));
            };
            [await tag("SHA-1"), await tag({ name: "SHA-256" })].join(" ")
        "#).await;
        assert_eq!(tags, "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subtle_keeps_key_bytes_off_the_key_and_refuses_what_it_does_not_do() {
        let out = with_crypto(r#"
            const key = await crypto.subtle.importKey("raw", new Uint8Array([1, 2, 3]), { name: "HMAC", hash: "SHA-256" }, true, ["verify"]);
            const refused = async (f) => { try { await f(); return "accepted"; } catch (e) { return e.name; } };
            [
              JSON.stringify(key),
              key instanceof CryptoKey,
              await refused(() => crypto.subtle.sign("HMAC", key, new Uint8Array(1))),
              await refused(() => crypto.subtle.importKey("jwk", {}, { name: "HMAC", hash: "SHA-256" }, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "MD5" }, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-1" }, false, ["encrypt"])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array(0), { name: "HMAC", hash: "SHA-1" }, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-1" }, false, [])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC" }, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("raw", new Uint8Array([1, 2, 3]), { name: "HMAC", hash: "SHA-1", length: 16 }, false, ["sign"])),
              (await crypto.subtle.importKey("raw", new Uint8Array([1, 2, 3]), { name: "hmac", hash: "sha-256", length: 20 }, false, ["sign"])).algorithm.length,
              await refused(() => crypto.subtle.sign("HMAC", { type: "secret", usages: ["sign"], algorithm: { name: "HMAC", hash: { name: "SHA-1" } } }, new Uint8Array(1))),
              await refused(async () => { try { key.usages.push("sign"); } catch {} return crypto.subtle.sign("HMAC", key, new Uint8Array(1)); }),
              await refused(() => crypto.subtle.sign("ECDSA", key, new Uint8Array(1))),
              await refused(() => new CryptoKey()),
            ].join(" | ")
        "#).await;
        assert_eq!(out, r#"{"type":"secret","extractable":true,"algorithm":{"name":"HMAC","hash":{"name":"SHA-256"},"length":24},"usages":["verify"]} | true | InvalidAccessError | NotSupportedError | NotSupportedError | SyntaxError | DataError | SyntaxError | TypeError | DataError | 20 | InvalidAccessError | InvalidAccessError | InvalidAccessError | TypeError"#);
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
