//! How moss builds a reqwest client. [`build_client`] is the way for an outside
//! host: the system proxy ([`crate::system::proxy`]) per request, and the
//! platform's CA roots with a fallback. [`client_builder`] and [`client`] are for
//! a client without that proxy. All three install the TLS crypto provider.

use crate::system::proxy::resolve_proxy_for_url;

/// A `reqwest::Proxy` that matches the system/browser proxy PER request.
///
/// reqwest calls this closure for every request URL — even when the `Client`
/// is cached — so it follows a live proxy change (the user turning their VPN /
/// Clash / V2Ray on or off) with no caching, exactly mirroring the ureq
/// `proxied_agent` path; [`build_client`] sets it, so behind the GFW a request
/// routes through the proxy instead of a reset direct connection. A
/// resolved proxy string that fails to parse falls through to `None` (direct),
/// the same graceful degradation ureq's builder does.
pub fn system_proxy() -> reqwest::Proxy {
    reqwest::Proxy::custom(|url| {
        resolve_proxy_for_url(url.as_str()).and_then(|p| reqwest::Url::parse(&p).ok())
    })
}

/// Builds a reqwest client for an outside host: the system proxy per request
/// and the platform's CA roots, or Mozilla's roots when the platform has none.
///
/// On Linux the platform roots are files (the `ca-certificates` package); a
/// minimal container or server image without them makes reqwest's builder fail
/// with "No CA certificates were loaded from the system". macOS and Windows ask
/// the OS and never take the fallback. `configure` runs once per attempt,
/// since `build` consumes the builder.
pub fn build_client(
    configure: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder,
) -> reqwest::Result<reqwest::Client> {
    let builder = || configure(client_builder().proxy(system_proxy()));
    builder().build().or_else(|platform_error| {
        #[cfg(target_os = "linux")]
        if let Ok(client) = builder().tls_certs_only(bundled_roots()).build() {
            log::warn!("no usable system CA certificates ({platform_error}); using the bundled Mozilla roots");
            return Ok(client);
        }
        Err(platform_error)
    })
}

/// `reqwest::Client::builder()` with the crypto provider installed and without
/// [`system_proxy`] (reqwest's own proxy variables still apply), for a probe that
/// must go direct, such as a captive-portal check. Anything else uses
/// [`build_client`].
pub fn client_builder() -> reqwest::ClientBuilder {
    install_crypto_provider();
    reqwest::Client::builder()
}

/// `reqwest::Client::new()` with the crypto provider installed: a
/// [`client_builder`] client with reqwest's defaults.
pub fn client() -> reqwest::Client {
    client_builder().build().expect("reqwest client with default settings")
}

/// Makes ring the process's TLS crypto provider, unless one is installed.
///
/// reqwest is built without a provider of its own and panics on `build` when
/// the process has none. [`client_builder`] calls this; a process that builds
/// clients some other way (an updater, a crash reporter) calls it once at
/// startup.
pub fn install_crypto_provider() {
    // Err means a provider is already installed, which is all we need.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(target_os = "linux")]
fn bundled_roots() -> impl Iterator<Item = reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|der| reqwest::Certificate::from_der(der.as_ref()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`build_client`] builds with no provider installed beforehand (reqwest has
    /// none of its own and panics) and with [`system_proxy`] set; reqwest calls
    /// the proxy closure per request, so construction checks only its wiring.
    #[test]
    fn client_builds_with_system_proxy() {
        let client = build_client(|b| b);
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
