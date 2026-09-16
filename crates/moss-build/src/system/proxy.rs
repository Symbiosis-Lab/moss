//! Resolve the proxy the SYSTEM (and thus the WKWebView) uses for a given URL.
//!
//! moss's Rust HTTP clients (ureq in `plugins/runtime/download.rs`, and reqwest
//! in the seta/deploy client + setup probes) otherwise connect DIRECTLY, so
//! behind a split-tunnel VPN / GFW proxy they cannot reach hosts the browser
//! reaches through the proxy — e.g. `server.matters.town/graphql` times out
//! while `matters.town` loads fine in the login webview (article sync fails),
//! and a direct connection to `api.mosspub.com` (Cloudflare) is reset so every
//! deploy `sync`/`upload`/`commit` and "Send logs" call fails.
//!
//! On macOS we call **`CFNetworkCopyProxiesForURL`** — the exact API WKWebView
//! uses to pick a proxy for a URL. Unlike the static system proxy (`scutil
//! --proxy`), it honors per-URL split-tunnel rules, host exceptions, and PAC
//! files, so moss matches the browser's actual routing. Other platforms fall
//! back to the standard proxy env vars (which reqwest already honors elsewhere).
//!
//! Two consumers, one resolver:
//!   * ureq builds an `ureq::Proxy` from the [`resolve_proxy_for_url`] string.
//!   * reqwest wraps the same resolver in `system_proxy` (`system/proxy_reqwest.rs`) — a
//!     `reqwest::Proxy::custom` that reqwest re-invokes PER request URL, so a
//!     proxy change is followed with no caching even when the `Client` is
//!     cached.
//!
//! Returns a proxy URL string both understand (`http://host:port` or
//! `socks5://host:port`), or `None` for a direct connection.
//!
//! Loopback URLs and anything in `NO_PROXY` never reach either resolver — see
//! [`bypasses_proxy`].

/// Build a ureq agent that routes through the proxy resolved for `url`
/// (per-URL system proxy on macOS, `HTTPS_PROXY`-style env elsewhere).
/// Without this, ureq connects directly and times out on hosts only
/// reachable through the proxy (split-tunnel VPNs, GFW users, sandboxed
/// environments).
pub fn proxied_ureq_agent(url: &str, timeout: std::time::Duration) -> ureq::Agent {
    // ureq's own default (`AgentBuilder::redirects` doc) — spelled out now
    // that a second caller needs a different value, not a behavior change.
    proxied_agent_builder(url, timeout, 5).build()
}

/// [`proxied_ureq_agent`] with automatic redirect-following disabled — for a
/// caller that must re-validate every hop's target itself before following
/// it (SSRF: a validated public host can 302/303/307/308 to an
/// internal/loopback address the caller's own check never sees). See
/// `vault::import::scrape::run::refuse_unsafe_scrape_url` and its
/// redirect-following caller, the only consumer today.
///
/// A 3xx response comes back as `Ok` with the redirect status intact
/// (ureq's documented `redirects(0)` behavior) rather than as an `Err`, so
/// the caller can read its `Location` header and decide.
pub fn proxied_ureq_agent_no_redirects(url: &str, timeout: std::time::Duration) -> ureq::Agent {
    proxied_agent_builder(url, timeout, 0).build()
}

/// Shared proxy-resolution setup for both agent constructors above — the
/// only difference between them is `redirects`.
fn proxied_agent_builder(
    url: &str,
    timeout: std::time::Duration,
    redirects: u32,
) -> ureq::AgentBuilder {
    let mut builder = ureq::AgentBuilder::new().timeout(timeout).redirects(redirects);
    if let Some(proxy_url) = resolve_proxy_for_url(url) {
        match ureq::Proxy::new(&proxy_url) {
            Ok(proxy) => {
                log::debug!("http: routing {} via system proxy {}", url, proxy_url);
                builder = builder.proxy(proxy);
            }
            Err(e) => log::warn!(
                "http: resolved proxy '{}' is invalid ({}); connecting directly",
                proxy_url,
                e
            ),
        }
    }
    builder
}

/// The proxy moss's HTTP clients should use for `url`, or `None` for a direct
/// connection.
pub fn resolve_proxy_for_url(url: &str) -> Option<String> {
    if bypasses_proxy(url, no_proxy_env().as_deref()) {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        macos::resolve(url)
    }
    #[cfg(not(target_os = "macos"))]
    {
        env_fallback(url)
    }
}

/// The user's `NO_PROXY` list, if any.
fn no_proxy_env() -> Option<String> {
    ["NO_PROXY", "no_proxy"]
        .iter()
        .find_map(|v| std::env::var(v).ok())
        .filter(|s| !s.trim().is_empty())
}

/// Should `url` skip the proxy entirely? Asked once, before either platform
/// resolver, because both get it wrong on their own: macOS `CFNetworkCopyProxiesForURL`
/// happily routes `http://<cid>.ipfs.localhost:8081/` through the user's proxy
/// (a live local gateway then reads as a 503), and the env branch never looked
/// at `NO_PROXY` at all.
///
/// `no_proxy` is passed in rather than read here so the predicate stays pure.
fn bypasses_proxy(url: &str, no_proxy: Option<&str>) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host() else {
        return false;
    };
    if is_loopback(&host) {
        return true;
    }
    match no_proxy {
        Some(list) => no_proxy_matches(list, &host_key(&host), parsed.port_or_known_default()),
        None => false,
    }
}

/// The host as the `NO_PROXY` list spells it: lowercase, no root dot, IPs bare.
fn host_key(host: &url::Host<&str>) -> String {
    match host {
        url::Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    }
}

/// RFC 6761 reserves `localhost` and everything under it for the loopback
/// interface, so the suffix match is on `.localhost` — `notlocalhost` and
/// `evil.localhost.example.com` are ordinary internet hosts and must not match.
///
/// Outbound routing only, and deliberately wider than
/// [`crate::ops::serve::trust_boundary::host_is_loopback`], which decides which
/// `Host:` headers the preview server will answer. Naming an extra name here
/// only keeps a request off the user's proxy; naming one there opens the
/// server to it. They are not twins — do not unify them.
fn is_loopback(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// Split a `NO_PROXY` entry into host pattern and optional port pin. A bare
/// IPv6 literal carries colons of its own, so only an unambiguous single colon
/// (or a bracketed literal) is read as a port.
fn split_entry_port(entry: &str) -> (&str, Option<u16>) {
    if let Some(rest) = entry.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((h, tail)) => (h, tail.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (entry, None),
        };
    }
    if entry.matches(':').count() == 1 {
        if let Some((h, p)) = entry.split_once(':') {
            if let Ok(port) = p.parse::<u16>() {
                return (h, Some(port));
            }
        }
    }
    (entry, None)
}

/// Does `host` (with `port`) match the comma-separated `NO_PROXY` list?
/// `*` bypasses everything; a leading `.` or `*.` on an entry is the usual
/// spelling of "and its subdomains", which a bare entry already means.
fn no_proxy_matches(list: &str, host: &str, port: Option<u16>) -> bool {
    for raw in list.split(',') {
        let entry = raw.trim().to_ascii_lowercase();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return true;
        }
        let (pattern, entry_port) = split_entry_port(&entry);
        if entry_port.is_some() && entry_port != port {
            continue;
        }
        let pattern = pattern
            .strip_prefix("*.")
            .unwrap_or(pattern)
            .trim_start_matches('.');
        if pattern.is_empty() {
            continue;
        }
        if host == pattern || host.ends_with(&format!(".{pattern}")) {
            return true;
        }
    }
    false
}

/// Standard proxy env-var resolution (non-macOS).
#[cfg(not(target_os = "macos"))]
fn env_fallback(url: &str) -> Option<String> {
    let is_https = url.starts_with("https:");
    let vars: &[&str] = if is_https {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    vars.iter()
        .find_map(|v| std::env::var(v).ok())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
    use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease, CFTypeRef};
    use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
    use core_foundation_sys::string::CFStringRef;
    use core_foundation_sys::url::CFURLRef;
    use std::os::raw::c_void;

    #[link(name = "CFNetwork", kind = "framework")]
    extern "C" {
        fn CFNetworkCopySystemProxySettings() -> CFDictionaryRef;
        fn CFNetworkCopyProxiesForURL(url: CFURLRef, proxy_settings: CFDictionaryRef)
            -> CFArrayRef;
        static kCFProxyTypeKey: CFStringRef;
        static kCFProxyTypeNone: CFStringRef;
        static kCFProxyTypeHTTP: CFStringRef;
        static kCFProxyTypeHTTPS: CFStringRef;
        static kCFProxyTypeSOCKS: CFStringRef;
        static kCFProxyHostNameKey: CFStringRef;
        static kCFProxyPortNumberKey: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateWithString(
            allocator: core_foundation_sys::base::CFAllocatorRef,
            url_string: CFStringRef,
            base_url: CFURLRef,
        ) -> CFURLRef;
    }

    /// Value-equality of two `CFStringRef`s (both borrowed / GET-rule).
    fn cfstr_eq(a: CFStringRef, b: CFStringRef) -> bool {
        if a.is_null() || b.is_null() {
            return false;
        }
        let sa = unsafe { CFString::wrap_under_get_rule(a) };
        let sb = unsafe { CFString::wrap_under_get_rule(b) };
        sa == sb
    }

    /// Read a String value out of a proxy dict (GET rule for the value).
    fn dict_str(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
        let v = unsafe { CFDictionaryGetValue(dict, key as *const c_void) } as CFTypeRef;
        if v.is_null() {
            return None;
        }
        unsafe { CFType::wrap_under_get_rule(v) }
            .downcast::<CFString>()
            .map(|s| s.to_string())
    }

    /// Read an i64 (port) value out of a proxy dict.
    fn dict_i64(dict: CFDictionaryRef, key: CFStringRef) -> Option<i64> {
        let v = unsafe { CFDictionaryGetValue(dict, key as *const c_void) } as CFTypeRef;
        if v.is_null() {
            return None;
        }
        unsafe { CFType::wrap_under_get_rule(v) }
            .downcast::<CFNumber>()
            .and_then(|n| n.to_i64())
    }

    pub fn resolve(url_str: &str) -> Option<String> {
        let cf_str = CFString::new(url_str);
        let url_ref = unsafe {
            CFURLCreateWithString(
                kCFAllocatorDefault,
                cf_str.as_concrete_TypeRef(),
                std::ptr::null(),
            )
        };
        if url_ref.is_null() {
            return None;
        }
        let settings = unsafe { CFNetworkCopySystemProxySettings() };
        if settings.is_null() {
            unsafe { CFRelease(url_ref as CFTypeRef) };
            return None;
        }
        // Create-rule returns: we own url_ref, settings, and proxies.
        let proxies = unsafe { CFNetworkCopyProxiesForURL(url_ref, settings) };
        unsafe {
            CFRelease(url_ref as CFTypeRef);
            CFRelease(settings as CFTypeRef);
        }
        if proxies.is_null() {
            return None;
        }

        let count = unsafe { CFArrayGetCount(proxies) };
        let mut result = None;
        for i in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(proxies, i) } as CFDictionaryRef;
            if dict.is_null() {
                continue;
            }
            let ptype =
                unsafe { CFDictionaryGetValue(dict, kCFProxyTypeKey as *const c_void) } as CFStringRef;
            if ptype.is_null() {
                continue;
            }

            // Proxies come in preference order. A "None" entry means "connect
            // directly for this URL" — honor it and stop.
            if cfstr_eq(ptype, unsafe { kCFProxyTypeNone }) {
                break;
            }

            let scheme = if cfstr_eq(ptype, unsafe { kCFProxyTypeHTTP })
                || cfstr_eq(ptype, unsafe { kCFProxyTypeHTTPS })
            {
                // Both are reached over an HTTP CONNECT tunnel; ureq's http proxy
                // handles CONNECT for https targets.
                "http"
            } else if cfstr_eq(ptype, unsafe { kCFProxyTypeSOCKS }) {
                "socks5"
            } else {
                // Auto-config (PAC) or unknown type — skip; a later entry (or the
                // direct default) applies. Full PAC support would need
                // CFNetworkExecuteProxyAutoConfigurationURL.
                continue;
            };

            if let (Some(host), Some(port)) = (
                dict_str(dict, unsafe { kCFProxyHostNameKey }),
                dict_i64(dict, unsafe { kCFProxyPortNumberKey }),
            ) {
                result = Some(format!("{scheme}://{host}:{port}"));
                break;
            }
        }
        unsafe { CFRelease(proxies as CFTypeRef) };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bypasses(url: &str) -> bool {
        bypasses_proxy(url, None)
    }

    #[test]
    fn loopback_hosts_never_go_through_a_proxy() {
        assert!(bypasses("http://localhost:8081/"));
        assert!(bypasses("http://LocalHost/"));
        assert!(bypasses("http://localhost./"));
        // The live bug: an IPFS subdomain gateway.
        assert!(bypasses("http://bafyfoo.ipfs.localhost:8081/"));
        assert!(bypasses("http://127.0.0.1:1420/"));
        assert!(bypasses("http://127.4.5.6/"));
        assert!(bypasses("http://[::1]:1420/"));
    }

    #[test]
    fn hosts_that_merely_look_local_still_use_the_proxy() {
        assert!(!bypasses("https://notlocalhost/"));
        assert!(!bypasses("https://evil.localhost.example.com/"));
        assert!(!bypasses("https://api.mosspub.com/"));
        assert!(!bypasses("https://128.0.0.1/"));
    }

    #[test]
    fn a_url_with_no_host_is_not_a_bypass() {
        assert!(!bypasses("not a url"));
        assert!(!bypasses("file:///tmp/x"));
    }

    #[test]
    fn no_proxy_list_is_honored() {
        let no = Some("example.com, .internal.test");
        assert!(bypasses_proxy("https://example.com/x", no));
        assert!(bypasses_proxy("https://api.example.com/x", no));
        assert!(bypasses_proxy("https://db.internal.test/", no));
        assert!(!bypasses_proxy("https://notexample.com/", no));
        assert!(!bypasses_proxy("https://example.com.evil.net/", no));
        assert!(bypasses_proxy("https://anything.at.all/", Some("*")));
    }

    #[test]
    fn no_proxy_port_pin_only_matches_that_port() {
        let no = Some("example.com:8080");
        assert!(bypasses_proxy("http://example.com:8080/", no));
        assert!(!bypasses_proxy("http://example.com/", no));
        // A bare IPv6 literal's colons are not a port.
        assert!(bypasses_proxy("https://[fd00::1]/", Some("fd00::1")));
        assert!(bypasses_proxy("https://[fd00::1]:8443/", Some("[fd00::1]:8443")));
        assert!(!bypasses_proxy("https://[fd00::1]:443/", Some("[fd00::1]:8443")));
    }
}
