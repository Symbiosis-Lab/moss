//! `crypto.subtle` for plugins: the keys a plugin holds, kept in its
//! context's own table, and the Web Crypto shapes over them.

use rquickjs::{Ctx, Result, prelude::Func};
use std::cell::RefCell;
use std::rc::Rc;

/// crypto.randomUUID, and the part of `crypto.subtle` plugins sign requests
/// with: HMAC keys imported raw; ECDSA P-256 keys, generated, exported as a
/// public JWK or a PKCS#8 private key and imported from PKCS#8, which an OAuth
/// sign-in's DPoP proofs need; `sign` with either, and `digest`. Shaped as the
/// Web Crypto API is in browsers, Node and Deno, so the same plugin code runs in
/// moss and under a test runner. A signature is IEEE P1363 (r ‖ s), as Web
/// Crypto returns it, not DER.
///
/// Each key lives in this context's own table, and the script holds only its
/// index, so nothing a plugin's code does to its realm (replacing
/// `WeakMap.prototype.get`, say) can read a key's bytes, export a key made not
/// extractable, or sign with a key that may not sign. A private key made
/// extractable leaves when the plugin exports it: that is how a plugin keeps a
/// key in its secret store across restarts, and the plugin's choice.
pub fn install_crypto<'js>(ctx: &Ctx<'js>) -> Result<()> {
    use rquickjs::{Function, TypedArray, Value};
    let crypto = rquickjs::Object::new(ctx.clone())?;
    crypto.set("randomUUID", Func::from(|| uuid::Uuid::new_v4().to_string()))?;
    ctx.globals().set("crypto", crypto)?;
    let held: Rc<RefCell<Vec<HeldKey>>> = Default::default();

    let table = held.clone();
    let import_key = Function::new(ctx.clone(), move |ctx: Ctx<'js>, format: String, algorithm: String, param: String, data: TypedArray<'js, u8>, extractable: bool, usages: Value<'js>| -> Result<Value<'js>> {
        let data: &[u8] = data.as_ref();
        let key = match (format.as_str(), algorithm.as_str()) {
            ("raw", "HMAC") => {
                let Some((hash, algorithm)) = HASHES.iter().find(|(name, _)| *name == param) else {
                    return refuse(&ctx, "NotSupportedError", &format!("unsupported hash {param}"));
                };
                let Some((signs, verifies)) = read_usages(&ctx, usages, &["sign", "verify"]).filter(|(s, v)| *s || *v) else {
                    return refuse(&ctx, "SyntaxError", "an HMAC key's usages are sign and verify");
                };
                if data.is_empty() {
                    return refuse(&ctx, "DataError", "an HMAC key cannot be empty");
                }
                HeldKey::Hmac { key: ring::hmac::Key::new(*algorithm, data), hash, bits: data.len() * 8, signs, verifies, extractable }
            }
            ("pkcs8", "ECDSA") => {
                if param != "P-256" {
                    return refuse(&ctx, "NotSupportedError", &format!("moss's crypto.subtle has no ECDSA curve {param}; it has P-256"));
                }
                let Some((true, false)) = read_usages(&ctx, usages, &["sign"]) else {
                    return refuse(&ctx, "SyntaxError", "an ECDSA private key's only usage is sign");
                };
                let Ok(pair) = ring::signature::EcdsaKeyPair::from_pkcs8(P256, data, &ring::rand::SystemRandom::new()) else {
                    return refuse(&ctx, "DataError", "the data is not a P-256 private key in PKCS#8");
                };
                HeldKey::EcPrivate { pair, pkcs8: extractable.then(|| data.to_vec()) }
            }
            _ => return refuse(&ctx, "NotSupportedError", "moss's crypto.subtle imports raw HMAC keys and pkcs8 ECDSA keys only"),
        };
        let id = hold(&ctx, &table, key)?;
        answer(&ctx, &table.borrow(), id)
    })?;

    let table = held.clone();
    let generate_key = Function::new(ctx.clone(), move |ctx: Ctx<'js>, algorithm: String, curve: String, extractable: bool, usages: Value<'js>| -> Result<Value<'js>> {
        if algorithm != "ECDSA" {
            return refuse(&ctx, "NotSupportedError", "moss's crypto.subtle generates ECDSA P-256 keys only");
        }
        if curve != "P-256" {
            return refuse(&ctx, "NotSupportedError", &format!("moss's crypto.subtle has no ECDSA curve {curve}; it has P-256"));
        }
        // Web Crypto splits the usages: sign goes to the private key, verify to the public.
        let Some((true, verifies)) = read_usages(&ctx, usages, &["sign", "verify"]) else {
            return refuse(&ctx, "SyntaxError", "an ECDSA key pair's usages are sign and verify, and include sign");
        };
        let rng = ring::rand::SystemRandom::new();
        let made = ring::signature::EcdsaKeyPair::generate_pkcs8(P256, &rng)
            .ok()
            .and_then(|pkcs8| Some((ring::signature::EcdsaKeyPair::from_pkcs8(P256, pkcs8.as_ref(), &rng).ok()?, pkcs8)));
        let Some((pair, pkcs8)) = made else { return Err(rquickjs::Exception::throw_message(&ctx, "could not generate a P-256 key")) };
        let point = ring::signature::KeyPair::public_key(&pair).as_ref().to_vec();
        let private = hold(&ctx, &table, HeldKey::EcPrivate { pair, pkcs8: extractable.then(|| pkcs8.as_ref().to_vec()) })?;
        let public = hold(&ctx, &table, HeldKey::EcPublic { point, verifies })?;
        let pair = bare_object(&ctx)?;
        pair.set("privateKey", answer(&ctx, &table.borrow(), private)?)?;
        pair.set("publicKey", answer(&ctx, &table.borrow(), public)?)?;
        Ok(pair.into_value())
    })?;

    let table = held.clone();
    let sign = Function::new(ctx.clone(), move |ctx: Ctx<'js>, id: u32, algorithm: String, hash: String, data: TypedArray<'js, u8>| -> Result<Value<'js>> {
        let table = table.borrow();
        let signature = match (table.get(id as usize), algorithm.as_str()) {
            (Some(HeldKey::Hmac { key, signs: true, .. }), "HMAC") => ring::hmac::sign(key, data.as_ref()).as_ref().to_vec(),
            (Some(HeldKey::EcPrivate { .. }), "ECDSA") if hash != "SHA-256" => {
                return refuse(&ctx, "NotSupportedError", &format!("moss's crypto.subtle signs P-256 with SHA-256, not {hash}"));
            }
            (Some(HeldKey::EcPrivate { pair, .. }), "ECDSA") => match pair.sign(&ring::rand::SystemRandom::new(), data.as_ref()) {
                Ok(signature) => signature.as_ref().to_vec(),
                Err(_) => return Err(rquickjs::Exception::throw_message(&ctx, "could not sign")),
            },
            _ => return refuse(&ctx, "InvalidAccessError", &format!("the key is not a {algorithm} key that may sign")),
        };
        Ok(TypedArray::<u8>::new(ctx, signature)?.into_value())
    })?;

    let table = held;
    let export_key = Function::new(ctx.clone(), move |ctx: Ctx<'js>, id: u32, format: String| -> Result<Value<'js>> {
        export(&ctx, table.borrow().get(id as usize), &format)
    })?;

    let digest = Function::new(ctx.clone(), |ctx: Ctx<'js>, hash: String, data: TypedArray<'js, u8>| -> Result<Value<'js>> {
        let Some((_, hmac)) = HASHES.iter().find(|(name, _)| *name == hash) else {
            return refuse(&ctx, "NotSupportedError", &format!("unsupported digest {hash}"));
        };
        let sum = ring::digest::digest(hmac.digest_algorithm(), data.as_ref());
        Ok(TypedArray::<u8>::new(ctx, sum.as_ref().to_vec())?.into_value())
    })?;

    let install: Function = ctx.eval(SUBTLE_JS)?;
    install.call::<_, ()>((import_key, generate_key, sign, export_key, digest))?;
    Ok(())
}

/// `exportKey`, answered from the key's record: a public key as the x and y
/// of its JWK, an extractable private key as its PKCS#8.
fn export<'js>(ctx: &Ctx<'js>, key: Option<&HeldKey>, format: &str) -> Result<rquickjs::Value<'js>> {
    use base64::Engine as _;
    match (key, format) {
        (Some(HeldKey::EcPublic { point, verifies }), "jwk") => {
            // Uncompressed SEC 1 point: 0x04 ‖ x ‖ y, each half base64url without padding in a JWK.
            let (x, y) = point[1..].split_at(32);
            let jwk = bare_object(ctx)?;
            jwk.set("x", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(x))?;
            jwk.set("y", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(y))?;
            jwk.set("verifies", *verifies)?;
            Ok(jwk.into_value())
        }
        (Some(HeldKey::EcPrivate { pkcs8: Some(der), .. }), "pkcs8") => Ok(rquickjs::TypedArray::<u8>::new(ctx.clone(), der.clone())?.into_value()),
        (None | Some(HeldKey::EcPrivate { pkcs8: None, .. } | HeldKey::Hmac { extractable: false, .. }), _) | (_, "pkcs8") => {
            refuse(ctx, "InvalidAccessError", "the key is not an extractable key of that kind")
        }
        (Some(HeldKey::EcPublic { .. }), _) => refuse(ctx, "NotSupportedError", "moss's crypto.subtle exports a public key as jwk only"),
        (Some(_), _) => refuse(ctx, "NotSupportedError", "moss's crypto.subtle exports a private key as pkcs8 only, and no secret key"),
    }
}

/// A key a plugin holds. No `Debug`, so no log line can print one.
enum HeldKey {
    Hmac { key: ring::hmac::Key, hash: &'static str, bits: usize, signs: bool, verifies: bool, extractable: bool },
    /// Always may sign. `pkcs8` is kept only when the key is extractable.
    EcPrivate { pair: ring::signature::EcdsaKeyPair, pkcs8: Option<Vec<u8>> },
    EcPublic { point: Vec<u8>, verifies: bool },
}

const P256: &ring::signature::EcdsaSigningAlgorithm = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;

/// An object with no prototype, so no setter or getter a plugin put on
/// `Object.prototype` sees or changes what the host answers.
fn bare_object<'js>(ctx: &Ctx<'js>) -> Result<rquickjs::Object<'js>> {
    let object = rquickjs::Object::new(ctx.clone())?;
    object.set_prototype(None)?;
    Ok(object)
}

/// Everything the script's key object shows, read from the table once: the
/// index, type, algorithm, extractable and usages.
fn answer<'js>(ctx: &Ctx<'js>, table: &[HeldKey], id: u32) -> Result<rquickjs::Value<'js>> {
    let shown = bare_object(ctx)?;
    shown.set("id", id)?;
    let (kind, extractable, signs, verifies) = match &table[id as usize] {
        HeldKey::Hmac { hash, bits, signs, verifies, extractable, .. } => {
            shown.set("name", "HMAC")?;
            shown.set("hash", *hash)?;
            shown.set("length", *bits)?;
            ("secret", *extractable, *signs, *verifies)
        }
        HeldKey::EcPrivate { pkcs8, .. } => ("private", pkcs8.is_some(), true, false),
        HeldKey::EcPublic { verifies, .. } => ("public", true, false, *verifies),
    };
    if kind != "secret" {
        shown.set("name", "ECDSA")?;
        shown.set("namedCurve", "P-256")?;
    }
    shown.set("type", kind)?;
    shown.set("extractable", extractable)?;
    shown.set("signs", signs)?;
    shown.set("verifies", verifies)?;
    Ok(shown.into_value())
}

/// A refusal the script throws as an Error named `name`.
fn refuse<'js>(ctx: &Ctx<'js>, name: &str, message: &str) -> Result<rquickjs::Value<'js>> {
    rquickjs::IntoJs::into_js(format!("{name}: {message}"), ctx)
}

/// Whether `usages` include sign and verify, or None when it is not an array
/// of strings or names a usage outside `allowed`. Read once, so that one
/// reading decides both what the key may do and what it shows; a Proxy does
/// not convert, so its traps never run here.
fn read_usages<'js>(ctx: &Ctx<'js>, usages: rquickjs::Value<'js>, allowed: &[&str]) -> Option<(bool, bool)> {
    let usages = <Vec<String> as rquickjs::FromJs>::from_js(ctx, usages).ok()?;
    if usages.iter().any(|u| !allowed.contains(&u.as_str())) {
        return None;
    }
    Some((usages.iter().any(|u| u == "sign"), usages.iter().any(|u| u == "verify")))
}

/// Adds `key` to the table and answers its index.
fn hold(ctx: &Ctx<'_>, table: &RefCell<Vec<HeldKey>>, key: HeldKey) -> Result<u32> {
    let mut table = table.borrow_mut();
    let id = u32::try_from(table.len()).ok().filter(|_| table.len() < MAX_KEYS);
    let Some(id) = id else {
        return Err(rquickjs::Exception::throw_message(ctx, &format!("a plugin may hold at most {MAX_KEYS} keys")));
    };
    table.push(key);
    Ok(id)
}

/// Keys are never freed while the plugin runs, so their number (a generated
/// pair is two) is capped to bound the memory one plugin can hold.
const MAX_KEYS: usize = 65_536;

/// The hashes `digest` answers and an HMAC key may name.
const HASHES: [(&str, ring::hmac::Algorithm); 4] = [
    ("SHA-1", ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
    ("SHA-256", ring::hmac::HMAC_SHA256),
    ("SHA-384", ring::hmac::HMAC_SHA384),
    ("SHA-512", ring::hmac::HMAC_SHA512),
];

/// Wraps the host's keys and digest in the Web Crypto shapes. A key object
/// shows what the host answered for it; what it signs, whether it may sign and
/// whether it exports stay the host's, so changing those fields changes nothing.
/// A host refusal comes back as "Name: message" and is thrown as an Error of
/// that name, as the runtime has no DOMException.
const SUBTLE_JS: &str = r#"(importKey, generateKey, signWith, exportKey, digestOf) => {
  // Taken now, before any plugin code runs, so replacing them later cannot
  // redirect which index a key object stands for.
  const keys = new WeakMap();
  const { apply } = Reflect;
  const { get: mapGet, set: mapSet } = WeakMap.prototype;
  const { indexOf, slice } = String.prototype;
  const { isView } = ArrayBuffer;
  const nameOf = (a) => String(typeof a === "string" ? a : a?.name ?? "").toUpperCase();
  const error = (name, message) => Object.assign(new Error(message), { name });
  const host = (answer) => {
    if (typeof answer !== "string") return answer;
    const at = apply(indexOf, answer, [": "]);
    throw error(apply(slice, answer, [0, at]), apply(slice, answer, [at + 2]));
  };
  const bytes = (data) => {
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw new TypeError("expected an ArrayBuffer or a view of one");
  };
  // The key index a key object stands for, or one no key has.
  const idOf = (key) => {
    const id = apply(mapGet, keys, [key]);
    return typeof id === "number" ? id : 0xffffffff;
  };
  const curveOf = (algorithm) => {
    if (algorithm.namedCurve === undefined) throw new TypeError("an ECDSA key needs a namedCurve");
    return String(algorithm.namedCurve);
  };
  class CryptoKey {
    constructor() { throw new TypeError("Illegal constructor"); }
  }
  const value = (v) => ({ value: v, enumerable: true });
  const makeKey = (shown, length = shown.length) => {
    const usages = [];
    if (shown.signs) usages.push("sign");
    if (shown.verifies) usages.push("verify");
    const key = Object.create(CryptoKey.prototype, {
      type: value(shown.type),
      extractable: value(shown.extractable),
      algorithm: value(shown.name === "HMAC" ? { name: "HMAC", hash: { name: shown.hash }, length } : { name: shown.name, namedCurve: shown.namedCurve }),
      usages: value(usages),
    });
    apply(mapSet, keys, [key, shown.id]);
    return key;
  };
  const subtle = {
    async generateKey(algorithm, extractable, usages) {
      const name = nameOf(algorithm);
      const pair = host(generateKey(name, name === "ECDSA" ? curveOf(algorithm) : "", Boolean(extractable), usages));
      return { publicKey: makeKey(pair.publicKey), privateKey: makeKey(pair.privateKey) };
    },
    async importKey(format, keyData, algorithm, extractable, usages) {
      const name = nameOf(algorithm);
      let param = "";
      if (name === "HMAC") {
        if (algorithm.hash === undefined) throw new TypeError("an HMAC key needs a hash");
        param = nameOf(algorithm.hash);
      } else if (name === "ECDSA") {
        param = curveOf(algorithm);
      }
      // A format that takes no bytes (jwk) is refused by the host, not here.
      const data = keyData instanceof ArrayBuffer || isView(keyData) ? bytes(keyData) : new Uint8Array(0);
      let length;
      if (name === "HMAC" && algorithm.length !== undefined) {
        // The standard lets a key name a length within its last byte.
        const bits = data.length * 8;
        if (!(algorithm.length > bits - 8 && algorithm.length <= bits)) throw error("DataError", `length ${algorithm.length} does not fit a ${bits}-bit key`);
        length = algorithm.length;
      }
      return makeKey(host(importKey(String(format), name, param, data, Boolean(extractable), usages)), length);
    },
    async exportKey(format, key) {
      const out = host(exportKey(idOf(key), String(format)));
      if (isView(out)) return out.buffer;
      return { kty: "EC", crv: "P-256", x: out.x, y: out.y, ext: true, key_ops: out.verifies ? ["verify"] : [] };
    },
    async sign(algorithm, key, data) {
      const name = nameOf(algorithm);
      let hash = "";
      if (name === "ECDSA") {
        if (algorithm?.hash === undefined) throw new TypeError("ECDSA needs a hash");
        hash = nameOf(algorithm.hash);
      }
      return host(signWith(idOf(key), name, hash, bytes(data))).buffer;
    },
    async digest(algorithm, data) {
      return host(digestOf(nameOf(algorithm), bytes(data))).buffer;
    },
  };
  Object.defineProperty(globalThis.crypto, "subtle", { value: subtle, enumerable: true });
  globalThis.CryptoKey = CryptoKey;
}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::web_shims::{install_base64, install_text_codec};
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};

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
    async fn subtle_digests_as_the_standard_vectors_do() {
        // FIPS 180-2's "abc", and an unsupported name refused by its Web Crypto error name.
        let out = with_crypto(r#"
            const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");
            const abc = new TextEncoder().encode("..abc").subarray(2);
            let refused = "";
            try { await crypto.subtle.digest("MD5", abc); } catch (e) { refused = e.name; }
            [hex(await crypto.subtle.digest("SHA-256", abc)), hex(await crypto.subtle.digest({ name: "sha-1" }, abc)), refused].join(" ")
        "#).await;
        assert_eq!(out, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad a9993e364706816aba3e25717850c26c9cd0d89d NotSupportedError");
    }

    /// Code in the plugin's own realm that replaces the intrinsics the shim
    /// uses sees no key bytes and cannot make a verify-only key sign.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subtle_keys_survive_a_realm_that_replaces_its_intrinsics() {
        let out = with_crypto(r#"
            const seen = [];
            const { get, set } = WeakMap.prototype;
            WeakMap.prototype.set = function (k, v) { seen.push(v); return set.call(this, k, v); };
            WeakMap.prototype.get = function (k) { const v = get.call(this, k); seen.push(v); return typeof v === "object" && v ? { ...v, usages: ["sign"] } : v; };
            Array.prototype.includes = () => true;
            const key = await crypto.subtle.importKey("raw", new Uint8Array([7, 7, 7]), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
            const signed = await crypto.subtle.sign("HMAC", key, new Uint8Array(1)).then(() => "signed", (e) => e.name);
            const signer = await crypto.subtle.importKey("raw", new Uint8Array([9]), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
            WeakMap.prototype.get = () => 1;
            const forged = await crypto.subtle.sign("HMAC", {}, new Uint8Array(1)).then(() => "signed", (e) => e.name);
            const tricky = ["verify"];
            let reads = 0;
            tricky.some = () => ++reads === 2;
            const shown = await crypto.subtle.importKey("raw", new Uint8Array([5]), { name: "HMAC", hash: "SHA-256" }, false, tricky);
            const widened = await crypto.subtle.sign("HMAC", shown, new Uint8Array(1)).then(() => "signed", (e) => e.name);
            Object.defineProperty(Array.prototype, "0", { set() { Object.defineProperty(this, "0", { value: 1, writable: true, enumerable: true, configurable: true }); }, configurable: true });
            const swapped = await crypto.subtle.importKey("raw", new Uint8Array([3]), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
            delete Array.prototype["0"];
            const substituted = await crypto.subtle.sign("HMAC", swapped, new Uint8Array(1)).then(() => "signed", (e) => e.name);
            [signed, seen.length === 0, forged, widened, substituted, shown.usages.join(","), signer.usages.join(",")].join(" ")
        "#).await;
        assert_eq!(out, "InvalidAccessError true InvalidAccessError InvalidAccessError InvalidAccessError verify sign");
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
              await refused(() => crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new Uint8Array(1))),
              await refused(() => new CryptoKey()),
            ].join(" | ")
        "#).await;
        assert_eq!(out, r#"{"type":"secret","extractable":true,"algorithm":{"name":"HMAC","hash":{"name":"SHA-256"},"length":24},"usages":["verify"]} | true | InvalidAccessError | NotSupportedError | NotSupportedError | SyntaxError | DataError | SyntaxError | TypeError | DataError | 20 | InvalidAccessError | InvalidAccessError | InvalidAccessError | TypeError"#);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plugin_holding_the_most_keys_is_refused_another() {
        let out = with_crypto(r#"
            const imp = () => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
            for (let i = 0; i < 65536; i++) await imp();
            await imp().then(() => "accepted", (e) => `${e.name}: ${e.message}`)
        "#).await;
        assert_eq!(out, "Error: a plugin may hold at most 65536 keys");
    }

    /// Usages behind a Proxy are refused like any usages that are not a
    /// plain list, so no trap decides what a key may do.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn usages_behind_a_proxy_are_refused() {
        let out = with_crypto(r#"
            const imp = (usages) => crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-256" }, false, usages).then((k) => k.usages.join(","), (e) => e.name);
            let reads = 0;
            const flipping = new Proxy(["verify"], { get(t, p) { return p === "0" && ++reads > 1 ? "sign" : t[p]; } });
            const throwing = new Proxy(["sign"], { get(t, p) { if (p === "length") throw new Error("no"); return t[p]; } });
            [await imp(flipping), await imp(throwing), reads, await imp(["verify"])].join(" ")
        "#).await;
        assert_eq!(out, "SyntaxError SyntaxError 0 verify");
    }

    /// Each context has its own key table: a key one plugin imported does not
    /// sign in another's context, and the other's own first key signs with its
    /// own bytes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn keys_do_not_cross_contexts() {
        let rt = AsyncRuntime::new().unwrap();
        let (a, b) = (AsyncContext::full(&rt).await.unwrap(), AsyncContext::full(&rt).await.unwrap());
        let run = |code: &'static str| code.as_bytes();
        let held = async_with!(a => |ctx| {
            install_crypto(&ctx).unwrap();
            let promise: rquickjs::Promise = ctx.eval_promise(run(r#"globalThis.k = await crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);"#)).unwrap();
            let _: rquickjs::Value = promise.into_future().await.unwrap();
            rquickjs::Persistent::save(&ctx, ctx.globals().get::<_, rquickjs::Object>("k").unwrap())
        })
        .await;
        let out: String = async_with!(b => |ctx| {
            install_crypto(&ctx).unwrap();
            ctx.globals().set("foreign", held.restore(&ctx).unwrap()).unwrap();
            let promise: rquickjs::Promise = ctx.eval_promise(run(r#"
                const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");
                const own = await crypto.subtle.importKey("raw", new Uint8Array([2]), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
                const crossed = await crypto.subtle.sign("HMAC", foreign, new Uint8Array(1)).then(() => "signed", (e) => e.name);
                globalThis.out = [crossed, hex(await crypto.subtle.sign("HMAC", own, new Uint8Array(1)))].join(" ");
            "#)).unwrap();
            let _: rquickjs::Value = promise.into_future().await.unwrap();
            ctx.globals().get("out").unwrap()
        })
        .await;
        let own = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &[2]), &[0]);
        let hex: String = own.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(out, format!("InvalidAccessError {hex}"));
    }

    /// A P-256 key, a message and its signature, all made by Node's Web Crypto:
    /// a reference outside ring for what moss imports, signs and verifies.
    const NODE_PKCS8: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgRXxbtVRDZhIQFC4rueI95PgZB3JOly50sJ9REqYCpGChRANCAAQVUHciKTMLlII28nQaxuUC1vNv0TUspQOsZxmTGhdijhvrjIdU05v-Rv9BHWzQn3_bIHaVu-VTqoPCOvN8i3cD";
    const NODE_X: &str = "FVB3IikzC5SCNvJ0GsblAtbzb9E1LKUDrGcZkxoXYo4";
    const NODE_Y: &str = "G-uMh1TTm_5G_0EdbNCff9sgdpW75VOqg8I683yLdwM";
    const NODE_MESSAGE: &str = "moss signs a DPoP proof";
    const NODE_SIGNATURE: &str = "pwjGesl7JjEAKlwGQIcyRQDyluFXQRWOtPYg1dsK3eWxMP5jQbC5yLMKqLNObDi5BoAqlKbsaOGsF0BD4eI6hg";

    fn unb64url(s: &str) -> Vec<u8> {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).unwrap()
    }

    /// Verifies a 64-byte r ‖ s signature against a JWK's x and y.
    fn p256_verifies(x: &str, y: &str, message: &[u8], signature: &[u8]) -> bool {
        let point = [vec![4], unb64url(x), unb64url(y)].concat();
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point).verify(message, signature).is_ok()
    }

    /// Like `with_crypto`, with btoa so the script can answer bytes as base64url.
    async fn with_crypto_json(code: &'static str) -> serde_json::Value {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let text = async_with!(ctx => |ctx| {
            install_base64(&ctx).unwrap();
            install_crypto(&ctx).unwrap();
            install_text_codec(&ctx).unwrap();
            ctx.eval::<(), _>(r#"globalThis.b64url = (b) => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
                globalThis.unb64url = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));"#).unwrap();
            let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
            let envelope: rquickjs::Object = promise.into_future().await.unwrap();
            envelope.get::<_, String>("value").unwrap()
        })
        .await;
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_generated_p256_pair_signs_as_web_crypto_does() {
        let out = with_crypto_json(r#"
            const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
            const jwk = await crypto.subtle.exportKey("jwk", pair.publicKey);
            const message = new TextEncoder().encode("a DPoP proof");
            const signature = await crypto.subtle.sign({ name: "ECDSA", hash: { name: "SHA-256" } }, pair.privateKey, message);
            JSON.stringify({ jwk, public: pair.publicKey, private: pair.privateKey, isKey: pair.privateKey instanceof CryptoKey, signature: b64url(signature), length: signature.byteLength })
        "#).await;
        let jwk = &out["jwk"];
        assert_eq!((jwk["kty"].as_str(), jwk["crv"].as_str(), jwk["ext"].as_bool()), (Some("EC"), Some("P-256"), Some(true)));
        assert_eq!(jwk["key_ops"], serde_json::json!(["verify"]));
        assert_eq!(out["public"], serde_json::json!({ "type": "public", "extractable": true, "algorithm": { "name": "ECDSA", "namedCurve": "P-256" }, "usages": ["verify"] }));
        assert_eq!(out["private"], serde_json::json!({ "type": "private", "extractable": true, "algorithm": { "name": "ECDSA", "namedCurve": "P-256" }, "usages": ["sign"] }));
        assert_eq!(out["isKey"], true);
        assert_eq!(out["length"], 64, "Web Crypto answers r ‖ s, not DER");
        let (x, y) = (jwk["x"].as_str().unwrap(), jwk["y"].as_str().unwrap());
        assert!(p256_verifies(x, y, b"a DPoP proof", &unb64url(out["signature"].as_str().unwrap())));
        assert!(!p256_verifies(x, y, b"another proof", &unb64url(out["signature"].as_str().unwrap())));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_saved_pkcs8_key_signs_again_and_stays_unexported() {
        let out = with_crypto_json(r#"
            const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign"]);
            const jwk = await crypto.subtle.exportKey("jwk", pair.publicKey);
            const saved = b64url(await crypto.subtle.exportKey("pkcs8", pair.privateKey));
            const key = await crypto.subtle.importKey("pkcs8", unb64url(saved), { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
            const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new TextEncoder().encode("a later post"));
            const reexport = await crypto.subtle.exportKey("pkcs8", key).then(() => "exported", (e) => e.name);
            JSON.stringify({ jwk, saved, signature: b64url(signature), reexport, key, publicUsages: pair.publicKey.usages })
        "#).await;
        let saved = unb64url(out["saved"].as_str().unwrap());
        let node = unb64url(NODE_PKCS8);
        // Same PKCS#8 v1 shape as Web Crypto's export: header, private key, then the public key.
        assert_eq!((saved.len(), &saved[..36]), (node.len(), &node[..36]));
        let (x, y) = (out["jwk"]["x"].as_str().unwrap(), out["jwk"]["y"].as_str().unwrap());
        assert!(p256_verifies(x, y, b"a later post", &unb64url(out["signature"].as_str().unwrap())));
        assert_eq!(out["reexport"], "InvalidAccessError");
        assert_eq!(out["key"]["extractable"], false);
        assert_eq!(out["publicUsages"], serde_json::json!([]));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_key_node_made_imports_and_signs_for_its_public_key() {
        // Node's own signature verifies under Node's key, so the check below is sound.
        assert!(p256_verifies(NODE_X, NODE_Y, NODE_MESSAGE.as_bytes(), &unb64url(NODE_SIGNATURE)));
        let out = with_crypto_json(r#"
            const der = unb64url("MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgRXxbtVRDZhIQFC4rueI95PgZB3JOly50sJ9REqYCpGChRANCAAQVUHciKTMLlII28nQaxuUC1vNv0TUspQOsZxmTGhdijhvrjIdU05v-Rv9BHWzQn3_bIHaVu-VTqoPCOvN8i3cD");
            const key = await crypto.subtle.importKey("pkcs8", der.buffer, { name: "ECDSA", namedCurve: "P-256" }, true, ["sign"]);
            const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new TextEncoder().encode("moss signs a DPoP proof"));
            JSON.stringify({ signature: b64url(signature), back: b64url(await crypto.subtle.exportKey("pkcs8", key)) })
        "#).await;
        assert!(p256_verifies(NODE_X, NODE_Y, NODE_MESSAGE.as_bytes(), &unb64url(out["signature"].as_str().unwrap())));
        assert_eq!(out["back"], NODE_PKCS8);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ecdsa_refuses_what_it_does_not_do() {
        let out = with_crypto(r#"
            const refused = async (f) => { try { await f(); return "accepted"; } catch (e) { return e.name; } };
            const P256 = { name: "ECDSA", namedCurve: "P-256" };
            const pair = await crypto.subtle.generateKey(P256, false, ["sign", "verify"]);
            const open = await crypto.subtle.generateKey(P256, true, ["sign"]);
            const hmac = await crypto.subtle.importKey("raw", new Uint8Array([1]), { name: "HMAC", hash: "SHA-256" }, true, ["sign"]);
            [
              await refused(() => crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-384" }, true, ["sign"])),
              await refused(() => crypto.subtle.generateKey({ name: "ECDSA" }, true, ["sign"])),
              await refused(() => crypto.subtle.generateKey({ name: "RSA-PSS" }, true, ["sign"])),
              await refused(() => crypto.subtle.generateKey(P256, true, ["verify"])),
              await refused(() => crypto.subtle.generateKey(P256, true, ["sign", "encrypt"])),
              await refused(() => crypto.subtle.importKey("pkcs8", new Uint8Array([1, 2]), P256, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("pkcs8", new Uint8Array([1, 2]), P256, false, ["verify"])),
              await refused(() => crypto.subtle.importKey("pkcs8", new Uint8Array([1, 2]), P256, false, [])),
              await refused(() => crypto.subtle.importKey("jwk", {}, P256, false, ["sign"])),
              await refused(() => crypto.subtle.importKey("pkcs8", new Uint8Array([1]), { name: "ECDSA", namedCurve: "P-521" }, false, ["sign"])),
              await refused(() => crypto.subtle.exportKey("pkcs8", pair.privateKey)),
              await refused(() => crypto.subtle.exportKey("jwk", open.privateKey)),
              await refused(() => crypto.subtle.exportKey("pkcs8", pair.publicKey)),
              await refused(() => crypto.subtle.exportKey("raw", hmac)),
              await refused(() => crypto.subtle.exportKey("pkcs8", hmac)),
              await refused(() => crypto.subtle.sign({ name: "ECDSA", hash: "SHA-384" }, pair.privateKey, new Uint8Array(1))),
              await refused(() => crypto.subtle.sign("ECDSA", pair.privateKey, new Uint8Array(1))),
              await refused(() => crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.publicKey, new Uint8Array(1))),
              await refused(() => crypto.subtle.sign("HMAC", pair.privateKey, new Uint8Array(1))),
              await refused(() => crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, hmac, new Uint8Array(1))),
              JSON.stringify(pair.privateKey),
            ].join(" | ")
        "#).await;
        assert_eq!(out, r#"NotSupportedError | TypeError | NotSupportedError | SyntaxError | SyntaxError | DataError | SyntaxError | SyntaxError | NotSupportedError | NotSupportedError | InvalidAccessError | NotSupportedError | InvalidAccessError | NotSupportedError | InvalidAccessError | NotSupportedError | TypeError | InvalidAccessError | InvalidAccessError | InvalidAccessError | {"type":"private","extractable":false,"algorithm":{"name":"ECDSA","namedCurve":"P-256"},"usages":["sign"]}"#);
    }

    /// Whether a request carries a DPoP proof that verifies against the JWK in
    /// its own header and names this request, with `nonce` and, for a call
    /// with a token, `ath` (the token's SHA-256) as the server expects.
    fn dpop_ok(req: &mockito::Request, htm: &str, nonce: Option<&str>) -> bool {
        use base64::Engine as _;
        let Some(proof) = req.header("dpop").first().and_then(|v| v.to_str().ok()) else { return false };
        let parts: Vec<&str> = proof.split('.').collect();
        let [header, claims, signature] = parts[..] else { return false };
        let header: serde_json::Value = serde_json::from_slice(&unb64url(header)).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&unb64url(claims)).unwrap();
        let jwk = &header["jwk"];
        let htu = format!("http://{}{}", req.header("host")[0].to_str().unwrap(), req.path());
        let access = req.header("authorization").first().and_then(|v| v.to_str().ok()?.strip_prefix("DPoP ").map(str::to_owned));
        let ath = access.map(|token| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, token.as_bytes())));
        header["typ"] == "dpop+jwt" && header["alg"] == "ES256" && jwk["kty"] == "EC" && jwk["crv"] == "P-256" && jwk.get("d").is_none()
            && claims["htm"] == htm && claims["htu"] == htu.as_str()
            && claims["nonce"].as_str() == nonce && claims["ath"].as_str() == ath.as_deref()
            && p256_verifies(jwk["x"].as_str().unwrap(), jwk["y"].as_str().unwrap(), format!("{}.{}", parts[0], parts[1]).as_bytes(), &unb64url(signature))
    }

    /// The Bluesky plugin's sign-in and posting, as far as DPoP goes, with the
    /// runtime's own shims against a stand-in server that behaves as bsky.social
    /// does: the first request is refused for want of a nonce, the retry must
    /// carry it, and a later call with the saved key must prove its token.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dpop_proofs_verify_at_a_server_that_asks_for_a_nonce() {
        let mut server = mockito::Server::new_async().await;
        let first = server.mock("POST", "/oauth/par").match_request(|r| dpop_ok(r, "POST", None))
            .with_status(400).with_header("dpop-nonce", "n1").with_body(r#"{"error":"use_dpop_nonce"}"#).create_async().await;
        let retry = server.mock("POST", "/oauth/par").match_request(|r| dpop_ok(r, "POST", Some("n1")))
            .with_status(201).with_header("dpop-nonce", "n1").with_body(r#"{"request_uri":"urn:ok"}"#).create_async().await;
        let post = server.mock("GET", "/xrpc/app.bsky.actor.getProfile").match_request(|r| dpop_ok(r, "GET", Some("n2")) && r.header("authorization").len() == 1)
            .with_status(200).with_body(r#"{"handle":"a.test"}"#).create_async().await;
        let script = format!(r#"(async () => {{
            const enc = new TextEncoder();
            const b64url = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
            const unb64url = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
            const sha256 = async (text) => b64url(new Uint8Array(await crypto.subtle.digest("SHA-256", enc.encode(text))));
            class Dpop {{
              constructor(key, jwk) {{ this.key = key; this.jwk = jwk; this.nonces = new Map(); }}
              async proof(method, url, access) {{
                const u = new URL(url);
                const claims = {{ jti: crypto.randomUUID(), htm: method, htu: `${{u.origin}}${{u.pathname}}`, iat: Math.floor(Date.now() / 1000) }};
                const nonce = this.nonces.get(u.origin);
                if (nonce) claims.nonce = nonce;
                if (access) claims.ath = await sha256(access);
                const body = `${{b64url(enc.encode(JSON.stringify({{ typ: "dpop+jwt", alg: "ES256", jwk: this.jwk }})))}}.${{b64url(enc.encode(JSON.stringify(claims)))}}`;
                const signature = await crypto.subtle.sign({{ name: "ECDSA", hash: "SHA-256" }}, this.key, enc.encode(body));
                return `${{body}}.${{b64url(new Uint8Array(signature))}}`;
              }}
              async fetch(url, init, access) {{
                const origin = new URL(url).origin;
                for (let attempt = 0; ; attempt++) {{
                  const headers = {{ ...init.headers, dpop: await this.proof(init.method, url, access) }};
                  if (access) headers.authorization = `DPoP ${{access}}`;
                  const r = await fetch(url, {{ ...init, headers }});
                  const nonce = r.headers.get("dpop-nonce");
                  const stale = nonce && nonce !== this.nonces.get(origin);
                  if (nonce) this.nonces.set(origin, nonce);
                  if (attempt === 0 && stale && (r.status === 400 || r.status === 401)) continue;
                  return r;
                }}
              }}
            }}
            const base = "{base}";
            const pair = await crypto.subtle.generateKey({{ name: "ECDSA", namedCurve: "P-256" }}, true, ["sign", "verify"]);
            const {{ kty, crv, x, y }} = await crypto.subtle.exportKey("jwk", pair.publicKey);
            const signIn = new Dpop(pair.privateKey, {{ kty, crv, x, y }});
            const par = await signIn.fetch(`${{base}}/oauth/par`, {{ method: "POST", headers: {{ "content-type": "application/x-www-form-urlencoded" }}, body: "client_id=x" }});
            const saved = b64url(new Uint8Array(await crypto.subtle.exportKey("pkcs8", pair.privateKey)));
            const key = await crypto.subtle.importKey("pkcs8", unb64url(saved), {{ name: "ECDSA", namedCurve: "P-256" }}, false, ["sign"]);
            const later = new Dpop(key, signIn.jwk);
            later.nonces.set(base, "n2");
            const profile = await later.fetch(`${{base}}/xrpc/app.bsky.actor.getProfile`, {{ method: "GET" }}, "access-token");
            return `${{par.status}} ${{(await par.json()).request_uri}} ${{profile.status}}`;
        }})()"#, base = server.url());
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let answer = async_with!(ctx => |ctx| {
            install_base64(&ctx).unwrap();
            install_crypto(&ctx).unwrap();
            install_text_codec(&ctx).unwrap();
            crate::engine::url_shim::install_url(&ctx).unwrap();
            crate::engine::fetch_shim::install_fetch(&ctx, crate::engine::fetch_shim::tests::bridge_declaring("local", &[&server.host_with_port()]), Default::default()).unwrap();
            let promise: rquickjs::Promise = ctx.eval(script.as_bytes().to_vec()).unwrap();
            promise.into_future::<String>().await.unwrap()
        })
        .await;
        assert_eq!(answer, "201 urn:ok 200");
        first.assert_async().await;
        retry.assert_async().await;
        post.assert_async().await;
    }

    /// The other direction: a key moss generated, whose PKCS#8 Node's Web
    /// Crypto imported and signed with, verifies under the JWK moss exported
    /// for it; and moss, importing that PKCS#8 again, signs for the same JWK.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn web_crypto_signs_with_a_key_moss_made() {
        let x = "EOcSfPDX8_e_Kk-mdiqz-easEbOMjePPiyHh4L-iOeE";
        let y = "BUxthxCeweWQ6WV_rkDgYjnZ6WLeP3CTQjTfo1kUZoY";
        let node_signature = "mbeRizVfH8mWiuUp3k2TiECkmh1OV0qux2eXx6sVSPwNV5a-HMcy5_hPiSUqoCXEhunuTLPA9QHrVCvySoHGAA";
        assert!(p256_verifies(x, y, b"node signs with a key moss made", &unb64url(node_signature)));
        let out = with_crypto_json(r#"
            const der = unb64url("MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgGXUvTftaLNW97k3G1Q00JQdoHyDJvX0j4WNabR38eaGhRANCAAQQ5xJ88Nfz978qT6Z2KrP55qwRs4yN48-LIeHgv6I54QVMbYcQnsHlkOllf65A4GI52eli3j9wk0I036NZFGaG");
            const key = await crypto.subtle.importKey("pkcs8", der, { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
            JSON.stringify({ signature: b64url(await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new TextEncoder().encode("moss again"))) })
        "#).await;
        assert!(p256_verifies(x, y, b"moss again", &unb64url(out["signature"].as_str().unwrap())));
    }
}
