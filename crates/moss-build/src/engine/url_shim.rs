//! WHATWG URL shim backed by url::Url (report 5 §2). Two-arg relative resolution
//! is mandatory (matters main.ts:967/1233/1417). Must THROW on bad input — all
//! three relative sites are try/catch-wrapped and rely on the throw to skip.

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
    obj.set("href", resolved.as_str())?;
    obj.set("pathname", resolved.path())?;
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
    Ok(())
}

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
            let threw: bool = ctx.eval("(() => { try { new URL('not a url'); return false; } catch { return true; } })()").unwrap();
            (href, pathname, threw)
        })
        .await;
        assert_eq!(href, "https://x.io/scale-compare.html");
        assert_eq!(pathname, "/p/q");
        assert!(threw);
    }
}
