//! WHATWG URL shim backed by url::Url. Two-argument relative resolution is
//! required, and bad input must throw: callers resolve links in a try/catch
//! and rely on the throw to skip one.

use rquickjs::{Ctx, Object, Result};

/// Named constructor fn — promotes the closure body to a named fn item so Rust
/// can generalize over the `'js` lifetime. (A closure returning `Object<'js>`
/// cannot satisfy the `for<'js>` HRTB because `Object` is invariant in `'js`.)
///
/// Supports:
///   new URL(input)          — absolute URL parse
///   new URL(input, base)    — RFC-3986 relative resolution against base
///
/// Throws a TypeError on any bad input, matching browser behaviour.
fn url_constructor<'js>(
    ctx: Ctx<'js>,
    input: String,
    base: rquickjs::prelude::Opt<String>,
) -> Result<Object<'js>> {
    let resolved = match base.0 {
        Some(b) => {
            let base_url = url::Url::parse(&b).map_err(|e| {
                rquickjs::Exception::throw_type(&ctx, &format!("Invalid base URL: {e}"))
            })?;
            base_url.join(&input).map_err(|e| {
                rquickjs::Exception::throw_type(&ctx, &format!("Invalid URL: {e}"))
            })?
        }
        None => url::Url::parse(&input).map_err(|e| {
            rquickjs::Exception::throw_type(&ctx, &format!("Invalid URL: {e}"))
        })?,
    };
    let obj = Object::new(ctx.clone())?;
    // The fields a browser's URL has, read the way the WHATWG URL standard
    // spells them: `protocol` keeps its colon, `search` and `hash` their
    // leading `?`/`#` (empty when absent), and `port` is empty when it is the
    // scheme's default.
    obj.set("href", resolved.as_str())?;
    obj.set("protocol", format!("{}:", resolved.scheme()))?;
    obj.set("username", resolved.username())?;
    obj.set("password", resolved.password().unwrap_or(""))?;
    obj.set("hostname", resolved.host_str().unwrap_or(""))?;
    let port = resolved.port().map(|p| p.to_string()).unwrap_or_default();
    let host = match (resolved.host_str(), port.is_empty()) {
        (Some(h), false) => format!("{h}:{port}"),
        (Some(h), true) => h.to_string(),
        (None, _) => String::new(),
    };
    obj.set("host", host)?;
    obj.set("port", port)?;
    obj.set("pathname", resolved.path())?;
    obj.set("search", resolved.query().map(|q| format!("?{q}")).unwrap_or_default())?;
    obj.set("hash", resolved.fragment().map(|f| format!("#{f}")).unwrap_or_default())?;
    obj.set("origin", resolved.origin().ascii_serialization())?;
    Ok(obj)
}

/// Install `URL` on globalThis as a constructable function.
///
/// `Function::new(...).with_constructor(true)` is required so QuickJS accepts
/// `new URL(...)` — a plain `Func::from` is callable but not constructable.
/// This mirrors how `TextEncoder`/`TextDecoder` are registered in `web_shims.rs`.
pub fn install_url(ctx: &Ctx<'_>) -> Result<()> {
    let ctor = rquickjs::Function::new(
        ctx.clone(),
        url_constructor
            as fn(
                Ctx<'_>,
                String,
                rquickjs::prelude::Opt<String>,
            ) -> Result<Object<'_>>,
    )?
    .with_constructor(true);
    ctx.globals().set("URL", ctor)?;
    let install: rquickjs::Function = ctx.eval(SEARCH_PARAMS_JS)?;
    install.call::<_, ()>((rquickjs::prelude::Func::from(form_parse), rquickjs::prelude::Func::from(form_serialize)))?;
    Ok(())
}

/// A query's name-value pairs, by the URL standard's form-urlencoded parser.
fn form_parse(query: String) -> Vec<Vec<String>> {
    url::form_urlencoded::parse(query.as_bytes()).map(|(k, v)| vec![k.into_owned(), v.into_owned()]).collect()
}

fn form_serialize(pairs: Vec<Vec<String>>) -> String {
    let mut out = url::form_urlencoded::Serializer::new(String::new());
    for pair in &pairs {
        out.append_pair(pair.first().map_or("", String::as_str), pair.get(1).map_or("", String::as_str));
    }
    out.finish()
}

/// `URLSearchParams` over the parser and serialiser above, and each URL's
/// `searchParams`: it and `search` stay in step with each other and with
/// `href`. A URL's other fields are values read once, at construction.
const SEARCH_PARAMS_JS: &str = r##"(parse, serialize) => {
  const lists = new WeakMap();
  const changed = new WeakMap();
  const query = (s) => (s.startsWith("?") ? s.slice(1) : s);
  // Every argument is a USVString: a lone surrogate becomes U+FFFD, as it must before it reaches the parser.
  const usv = (v) => String(v).replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "\uFFFD");
  class URLSearchParams {
    constructor(init = "") {
      let list;
      if (init === null) list = [];
      else if (typeof init === "object" && typeof init[Symbol.iterator] === "function") {
        list = [...init].map((pair) => {
          const p = [...pair];
          if (p.length !== 2) throw new TypeError("each pair must have a name and a value");
          return [usv(p[0]), usv(p[1])];
        });
      } else if (typeof init === "object") list = Object.keys(init).map((k) => [usv(k), usv(init[k])]);
      else list = parse(query(usv(init)));
      lists.set(this, list);
    }
    get size() { return lists.get(this).length; }
    append(name, value) { lists.get(this).push([usv(name), usv(value)]); changed.get(this)?.(); }
    delete(name, value) {
      const list = lists.get(this);
      const keep = list.filter(([k, v]) => k !== usv(name) || (value !== undefined && v !== usv(value)));
      list.splice(0, list.length, ...keep);
      changed.get(this)?.();
    }
    get(name) { const hit = lists.get(this).find(([k]) => k === usv(name)); return hit ? hit[1] : null; }
    getAll(name) { return lists.get(this).filter(([k]) => k === usv(name)).map(([, v]) => v); }
    has(name, value) { return lists.get(this).some(([k, v]) => k === usv(name) && (value === undefined || v === usv(value))); }
    set(name, value) {
      const list = lists.get(this);
      const at = list.findIndex(([k]) => k === usv(name));
      if (at < 0) list.push([usv(name), usv(value)]);
      else {
        list[at][1] = usv(value);
        const rest = list.filter(([k], i) => i <= at || k !== usv(name));
        list.splice(0, list.length, ...rest);
      }
      changed.get(this)?.();
    }
    sort() { lists.get(this).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)); changed.get(this)?.(); }
    forEach(fn, self) { for (const [k, v] of lists.get(this)) fn.call(self, v, k, this); }
    *entries() { for (const [k, v] of lists.get(this)) yield [k, v]; }
    *keys() { for (const [k] of lists.get(this)) yield k; }
    *values() { for (const [, v] of lists.get(this)) yield v; }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return serialize(lists.get(this)); }
  }
  const NativeURL = globalThis.URL;
  globalThis.URL = function URL(input, base) {
    if (!new.target) throw new TypeError("URL is a constructor: call it with new");
    const url = base === undefined ? new NativeURL(input) : new NativeURL(input, base);
    let search = url.search;
    const params = new URLSearchParams(search);
    const write = (q) => {
      search = q ? "?" + q : "";
      url.href = url.href.split("#")[0].split("?")[0] + search + url.hash;
    };
    changed.set(params, () => write(params.toString()));
    Object.defineProperty(url, "search", {
      enumerable: true,
      get: () => search,
      set: (value) => {
        const q = query(usv(value));
        lists.set(params, parse(q));
        write(q);
      },
    });
    url.searchParams = params;
    url.toString = () => url.href;
    return url;
  };
  globalThis.URLSearchParams = URLSearchParams;
}"##;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resolves_relative_and_throws_on_garbage() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let (href, pathname, threw): (String, String, bool) = async_with!(ctx => |ctx| {
            install_url(&ctx).unwrap();
            let href: String = ctx.eval(
                "new URL('../../scale-compare.html', 'https://x.io/a/b/c.html').href"
            ).unwrap();
            let pathname: String = ctx.eval("new URL('https://x.io/p/q').pathname").unwrap();
            assert_eq!(ctx.eval::<String, _>("String(new URL('HTTPS://X.IO/p'))").unwrap(), "https://x.io/p");
            let threw: bool = ctx.eval("(() => { try { new URL('not a url'); return false; } catch { return true; } })()").unwrap();
            (href, pathname, threw)
        })
        .await;
        assert_eq!(href, "https://x.io/scale-compare.html");
        assert_eq!(pathname, "/p/q");
        assert!(threw);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn carries_the_fields_a_browser_url_has() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let fields: Vec<String> = async_with!(ctx => |ctx| {
            install_url(&ctx).unwrap();
            ctx.eval(
                "(() => { const u = new URL('../x?q=1#top', 'https://a:b@host.example:8443/p/q/'); \
                  const m = new URL('mailto:ada@example.org'); \
                  return [u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.search, u.hash, u.origin, \
                          new URL('https://host.example:443/').port, m.protocol, m.host, m.origin]; })()"
            ).unwrap()
        })
        .await;
        assert_eq!(
            fields,
            ["https:", "a", "b", "host.example:8443", "host.example", "8443", "?q=1", "#top", "https://host.example:8443", "", "mailto:", "", "null"]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn search_params_read_and_write_as_the_url_standard_says() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let out: String = async_with!(ctx => |ctx| {
            install_url(&ctx).unwrap();
            ctx.eval(r#"
                const u = new URL("https://x.io/p?q=a+b%26c&tag=1&tag=2&empty&caf%C3%A9=%E2%9C%93#top");
                const p = u.searchParams;
                const read = [p.get("q"), p.getAll("tag").join(","), p.get("empty"), p.get("café"), String(p.get("none")), p.has("tag", "2"), p.size].join("|");
                p.set("tag", "3"); p.append("sp ace", "a/b~!'()*"); p.delete("empty");
                const written = [u.search, u.href, String(u)].join("|");
                const built = [
                  new URLSearchParams({ a: 1, b: "x y" }).toString(),
                  new URLSearchParams([["k", "v"], ["k", "w"]]).getAll("k").join(","),
                  [...new URLSearchParams("?x=1&y=2")].map(([k, v]) => k + v).join(","),
                  new URLSearchParams("b=2&a=1&a=0").toString(),
                  new URLSearchParams("%E2%9C%93%E2=%E2%41").toString(),
                  new URLSearchParams(null).toString(),
                  new URLSearchParams([["a", "\uD800"]]).toString(),
                  (() => { try { new URLSearchParams([["a"]]); return "accepted"; } catch (e) { return e.name; } })(),
                  (() => { try { URL("https://x.io"); return "accepted"; } catch (e) { return e.name; } })(),
                ].join("|");
                const v = new URL("https://x.io/?a=1#f");
                v.search = "b=2&b=3";
                const set = [v.searchParams.getAll("b").join(","), v.href, v.search].join("|");
                const q = new URLSearchParams("c=3&a=1&b=2&a=0");
                q.delete("a", "0"); q.sort();
                const walked = []; q.forEach((v, k) => walked.push(k + v));
                const rest = [[...q.keys()].join(""), [...q.values()].join(""), walked.join(","), q.toString()].join("|");
                [read, written, built, rest, set].join("\n")
            "#).unwrap()
        })
        .await;
        assert_eq!(out.lines().collect::<Vec<_>>(), [
            "a b&c|1,2||✓|null|true|5",
            "?q=a+b%26c&tag=3&caf%C3%A9=%E2%9C%93&sp+ace=a%2Fb%7E%21%27%28%29*|https://x.io/p?q=a+b%26c&tag=3&caf%C3%A9=%E2%9C%93&sp+ace=a%2Fb%7E%21%27%28%29*#top|https://x.io/p?q=a+b%26c&tag=3&caf%C3%A9=%E2%9C%93&sp+ace=a%2Fb%7E%21%27%28%29*#top",
            "a=1&b=x+y|v,w|x1,y2|b=2&a=1&a=0|%E2%9C%93%EF%BF%BD=%EF%BF%BDA||a=%EF%BF%BD|TypeError|TypeError",
            "abc|123|a1,b2,c3|a=1&b=2&c=3",
            "2,3|https://x.io/?b=2&b=3#f|?b=2&b=3",
        ]);
    }
}
