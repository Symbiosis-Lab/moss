//! The reqwest adapter over [`crate::system::proxy`]. It lived app-side until
//! ADR-078 so this crate carried no reqwest; the seta client crossed and
//! brought reqwest with it, so the adapter follows the caller.
//! Callers: the seta client, the domain orchestrator, and the setup probes.

use crate::system::proxy::resolve_proxy_for_url;

/// A `reqwest::Proxy` that matches the system/browser proxy PER request.
///
/// reqwest calls this closure for every request URL — even when the `Client`
/// is cached — so it follows a live proxy change (the user turning their VPN /
/// Clash / V2Ray on or off) with no caching, exactly mirroring the ureq
/// `proxied_agent` path. Drop it into any external-facing reqwest
/// `Client::builder()` via `.proxy(system_proxy())` so, behind the GFW, the
/// request routes through the proxy instead of a reset direct connection. A
/// resolved proxy string that fails to parse falls through to `None` (direct),
/// the same graceful degradation ureq's builder does.
pub fn system_proxy() -> reqwest::Proxy {
    reqwest::Proxy::custom(|url| {
        resolve_proxy_for_url(url.as_str()).and_then(|p| reqwest::Url::parse(&p).ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building a reqwest client with [`system_proxy`] set must succeed. This
    /// validates the `Proxy::custom` closure types end-to-end (and, transitively,
    /// that the `socks` feature compiles) without needing a live proxy — reqwest
    /// invokes the closure lazily per request, so construction alone exercises
    /// the wiring. This is the deterministic guard for the reqwest-proxy fix.
    #[test]
    fn client_builds_with_system_proxy() {
        let client = reqwest::Client::builder().proxy(system_proxy()).build();
        assert!(
            client.is_ok(),
            "reqwest client must build with the system proxy: {:?}",
            client.err()
        );
    }

    /// The resolver must be well-formed: when it resolves a proxy for a URL it
    /// must yield a parseable proxy URL, and it must not panic. On CI / a
    /// proxy-less dev box it returns `None` (a direct connection); a developer
    /// with a system proxy active may legitimately get `Some(..)`, so we assert
    /// parseability rather than a fixed `None` to stay environment-independent.
    #[test]
    fn resolve_proxy_is_well_formed() {
        let resolved = resolve_proxy_for_url("https://api.mosspub.com/");
        if let Some(p) = resolved {
            assert!(
                reqwest::Url::parse(&p).is_ok(),
                "resolved proxy must be a parseable URL: {p}"
            );
        }
    }
}
