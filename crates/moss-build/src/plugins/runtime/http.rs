//! HTTP transport shared by plugin helpers and the fetch shim.
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Fetch redirect policy, enforced before a second request can be sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Redirect {
    #[default]
    Follow,
    Error,
    Manual,
}

pub(super) enum Body {
    Text(String),
    Bytes(Vec<u8>),
}

fn validate(url: &url::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("Invalid URL scheme '{}': only http/https allowed", url.scheme()));
    }
    Ok(())
}

fn same_origin(a: &url::Url, b: &url::Url) -> bool {
    a.scheme() == b.scheme() && a.host() == b.host() && a.port_or_known_default() == b.port_or_known_default()
}

fn strip_cross_origin(headers: &mut HashMap<String, String>) {
    headers.retain(|name, _| matches!(name.as_str(), "accept" | "accept-language" | "content-type" | "user-agent"));
}

fn clear_userinfo(url: &mut url::Url) {
    let _ = url.set_username("");
    let _ = url.set_password(None);
}

/// Follow at most four hops, matching the pinned client's redirects(5) boundary.
/// Credentials are removed from the evolving header set, never copied back in.
pub(super) fn request(address: &str, mut method: &str, mut body: Option<Body>, headers: HashMap<String, String>, timeout: Duration, policy: Redirect) -> Result<ureq::Response, String> {
    use base64::Engine;
    let started = Instant::now();
    if address.is_empty() { return Err("URL cannot be empty".into()); }
    let mut url: url::Url = address.parse().map_err(|e| format!("Invalid URL: {e}"))?;
    validate(&url)?;
    let mut headers: HashMap<_, _> = headers.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect();
    headers.entry("user-agent".into()).or_insert_with(crate::system::user_agent);
    // Materialize initial URL credentials once so ureq cannot regenerate them
    // after a boundary. Location userinfo never introduces new credentials.
    if !url.username().is_empty() || url.password().is_some_and(|p| !p.is_empty()) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", url.username(), url.password().unwrap_or("")));
        headers.entry("authorization".into()).or_insert_with(|| format!("Basic {encoded}"));
    }
    clear_userinfo(&mut url);
    let remaining = || timeout.checked_sub(started.elapsed()).filter(|d| !d.is_zero()).ok_or_else(|| format!("Request timeout after {}ms", timeout.as_millis()));
    let mut followed = 0;
    loop {
        let agent = crate::system::proxy::proxied_ureq_agent_no_redirects(url.as_str(), remaining()?);
        let mut req = agent.request(method, url.as_str()).timeout(remaining()?);
        for (name, value) in &headers { req = req.set(name, value); }
        let response = match &body {
            Some(Body::Text(text)) => req.send_string(text),
            Some(Body::Bytes(bytes)) => req.send_bytes(bytes),
            None => req.call(),
        };
        let response = match response {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(format!("request failed: {e}")),
        };
        let status = response.status();
        if policy == Redirect::Error && (300..400).contains(&status) {
            return Err(format!("redirect refused (HTTP {status})"));
        }
        if policy != Redirect::Follow || !(300..399).contains(&status) { return Ok(response); }
        // The old limit was checked even before Location or method eligibility.
        if followed == 4 { return Err("reached max redirects (5)".into()); }
        let Some(location) = response.header("location") else { return Ok(response) };
        let mut next = url.join(location).map_err(|e| format!("Invalid redirect URL: {e}"))?;
        let next_method = match status {
            301 | 302 | 303 => if method == "HEAD" { "HEAD" } else { "GET" },
            307 | 308 if matches!(method, "GET" | "HEAD") => method,
            _ => return Ok(response),
        };
        validate(&next)?;
        if !same_origin(&url, &next) { strip_cross_origin(&mut headers); }
        clear_userinfo(&mut next);
        // Every followable request is now bodyless; let ureq supply framing.
        headers.remove("content-length");
        headers.remove("transfer-encoding");
        if body.take().is_some() { headers.remove("content-type"); }
        method = next_method;
        url = next;
        followed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_compare_scheme_normalized_host_and_effective_port() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(same_origin(&u("http://EXAMPLE.org/a"), &u("http://example.org:80/b")));
        assert!(same_origin(&u("https://example.org"), &u("https://example.org:443/")));
        assert!(!same_origin(&u("http://example.org"), &u("https://example.org")));
        assert!(!same_origin(&u("http://example.org"), &u("http://example.org:81")));
        assert!(!same_origin(&u("http://127.0.0.1"), &u("http://localhost")));
    }
}
