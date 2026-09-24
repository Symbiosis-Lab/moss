//! TCP port utilities for the preview server.
//!
//! Provides port availability checking, port scanning, and server readiness
//! verification used by the server lifecycle layer.
//!
//! ## Identity verification
//!
//! Foreign dev servers (eleventy, vite, jekyll) on the same port frequently
//! return HTTP 200 for `GET /`. To prevent moss from blindly "reusing" a
//! foreign server's port, [`verify_server_ready`] hits the dedicated
//! [`MOSS_HEALTH_PATH`] endpoint and requires a moss-specific marker token
//! ([`MOSS_HEALTH_MARKER`]) in the response body before accepting the port as
//! moss-owned. The matching route handler is defined in `router.rs`.

/// The reserved health-check path served by every moss preview server.
///
/// The trailing slash matters: `ServeDir` may otherwise interpret the bare
/// segment as a request for a static file under that name. Defining a real
/// `.route()` for this path with no trailing-slash fallback guarantees the
/// user's site cannot shadow it.
pub const MOSS_HEALTH_PATH: &str = "/__moss_health/";

/// Marker substring that the moss health endpoint embeds in its JSON body.
///
/// Used by [`verify_server_ready`] to confirm that the server responding on
/// the probed port is actually a moss preview server, not a stale eleventy /
/// vite / jekyll / next.js instance that happened to bind the same port.
pub const MOSS_HEALTH_MARKER: &str = "\"moss-preview-server\"";

/// Finds an available TCP port starting from the given port number.
///
/// Scans through a range of 100 ports starting from `start_port` to find
/// an available port for the preview server.
///
/// # Arguments
/// * `start_port` - Port number to start scanning from
///
/// # Returns
/// * `Ok(u16)` - First available port found
/// * `Err(String)` - No available ports in the scanned range
pub fn find_available_port(start_port: u16) -> Result<u16, String> {
    for port in start_port..start_port + 100 {
        if is_port_available(port) {
            return Ok(port);
        }
    }
    Err(format!(
        "No available ports found starting from {}",
        start_port
    ))
}

/// Checks if a TCP port is available for binding on both IPv4 and IPv6 loopback.
///
/// Probes both `127.0.0.1:{port}` and `[::1]:{port}`. Returns `false` if
/// either address family is already in use. This is critical because a
/// foreign dev server (e.g., eleventy bound on `[::]:8080`) may hold the
/// IPv6 wildcard while leaving IPv4 free — a single-family probe would
/// erroneously declare the port available, moss would bind IPv4, and the
/// `http://localhost:` iframe URL (IPv6-first on macOS) would silently route
/// to the foreign server.
///
/// This is defense-in-depth — the authoritative collision check is the
/// dual-stack server bind in `router.rs::start_server`.
/// We still want `is_port_available` to be honest so the
/// `if available { use } else { scan }` ladder picks the right starting
/// point.
///
/// # Arguments
/// * `port` - Port number to test
///
/// # Returns
/// * `true` - Port is available on both IPv4 and IPv6 loopback
/// * `false` - Port is already in use on at least one of the two
pub fn is_port_available(port: u16) -> bool {
    let v4_ok = std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
    let v6_ok = std::net::TcpListener::bind(("::1", port)).is_ok();
    v4_ok && v6_ok
}

/// The app's port base: `MOSS_PREVIEW_PORT_BASE` shifts the scan start so
/// Claude sessions can avoid colliding with a human's `pnpm run dev` (which
/// keeps the canonical 8080). The one environment read for the preview port —
/// `ServeConfig::new` takes the resolved value, never the env.
pub fn env_port_base() -> u16 {
    match std::env::var("MOSS_PREVIEW_PORT_BASE") {
        Ok(s) => match s.parse::<u16>() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("[moss] WARN: MOSS_PREVIEW_PORT_BASE='{}' is not a valid u16; falling back to 8080", s);
                8080
            }
        },
        Err(_) => 8080,
    }
}

/// Verifies that a moss preview server is ready and responding on the given port.
///
/// Sends an HTTP GET to [`MOSS_HEALTH_PATH`] on `127.0.0.1` (literal — not
/// `localhost` — to avoid IPv4/IPv6 DNS resolution drift) and requires the
/// response body to contain [`MOSS_HEALTH_MARKER`]. Any other response
/// (connection refused, timeout, foreign server's 200, missing marker)
/// results in `Err`.
///
/// This protects the server-reuse path from accepting a foreign dev server
/// as a moss server. Before this gating existed, the old check accepted
/// *any* HTTP response (incl. eleventy on the same port) as "moss is ready".
///
/// # Arguments
/// * `port` - Port number where the moss server should be running
///
/// # Returns
/// * `Ok(())` - A moss preview server is responding on this port
/// * `Err(String)` - No moss server detected (foreign server, no server, or unreachable)
pub async fn verify_server_ready(port: u16) -> Result<(), String> {
    // Use the IPv4 literal explicitly. `localhost` would expose us to IPv6-first
    // DNS resolution on macOS, which can route the probe to a foreign IPv6
    // server while moss is bound on IPv4 — the exact bug class this function
    // exists to prevent.
    let url = format!("http://127.0.0.1:{}{}", port, MOSS_HEALTH_PATH);
    let max_attempts = 10;
    let delay_ms = 100;

    for attempt in 1..=max_attempts {
        let url_clone = url.clone();
        let result = tokio::task::spawn_blocking(move || {
            ureq::get(&url_clone)
                .timeout(std::time::Duration::from_secs(1))
                .call()
                .map(|resp| resp.into_string().unwrap_or_default())
        })
        .await;

        match result {
            Ok(Ok(body)) => {
                if body.contains(MOSS_HEALTH_MARKER) {
                    return Ok(());
                }
                // 200 OK but not from moss — a foreign server is squatting
                // this port. Don't retry; the marker won't appear later.
                return Err(format!(
                    "Server on port {} responded but is not a moss preview server \
                     (health marker missing). A foreign server may be holding this port.",
                    port
                ));
            }
            Ok(Err(ureq::Error::Status(_status, _))) => {
                // 4xx/5xx from the health endpoint means a server is up but
                // it's not us. Reject immediately — same reasoning as above.
                return Err(format!(
                    "Server on port {} returned non-success status from {} \
                     (likely a foreign server, not moss)",
                    port, MOSS_HEALTH_PATH
                ));
            }
            Ok(Err(_)) | Err(_) => {
                // Connection refused / timeout — server may still be starting.
                // Sleep only after a failed attempt, not before the first check.
                if attempt < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    Err(format!(
        "Server failed to respond after {} attempts",
        max_attempts
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_port_available_function_runs() {
        // Test that the function can be called without panicking
        // We use a high port that's unlikely to be in use
        let _ = is_port_available(59123);
    }

    #[test]
    fn test_find_available_port_success() {
        // Should find an available port in the range
        let result = find_available_port(50000);
        assert!(result.is_ok(), "Should find an available port");
        let port = result.unwrap();
        assert!(port >= 50000, "Port should be >= start_port");
        assert!(port < 50100, "Port should be < start_port + 100");
    }

    #[test]
    fn test_find_available_port_returns_first_available() {
        // Find a port, then verify it's actually available by trying to bind to it
        let port = find_available_port(51000).expect("Should find a port");

        // Verify the port is actually bindable
        let listener = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(listener.is_ok(), "Returned port should be bindable");
    }

    #[test]
    fn test_is_port_available_holds_when_v4_bound() {
        // Half of the dual-stack contract: if IPv4 127.0.0.1:port is held,
        // is_port_available must return false even when IPv6 is free.
        // Sibling test below covers the IPv6-held case.
        let port = find_available_port(52000).expect("Should find a port");

        // It should be available
        assert!(is_port_available(port), "Found port should be available");

        // Bind to it
        let _listener =
            std::net::TcpListener::bind(("127.0.0.1", port)).expect("Should be able to bind");

        // Now it should NOT be available
        assert!(
            !is_port_available(port),
            "Bound port should not be available"
        );
    }

    #[test]
    #[ignore = "requires IPv6 ::1 bind capability; not all sandboxed CI runners grant it"]
    fn test_is_port_available_detects_ipv6_collision() {
        // Regression for the 2026-05-22 dual-stack-collision bug:
        // a foreign server holding only IPv6 [::1]:PORT must cause
        // is_port_available(PORT) to return false, even though IPv4
        // 127.0.0.1:PORT is technically still free. Before this fix,
        // an IPv4-only probe would erroneously return true and moss
        // would bind IPv4 alongside the foreign IPv6 server, causing
        // the iframe (IPv6-first via DNS) to silently hit the foreign
        // server's content.
        //
        // This test is `#[ignore]`-gated because the IPv6 loopback bind
        // capability is not universally available in sandboxed CI runners
        // (some deny `::1` bind entirely). Run locally with:
        //   cargo test -p moss --lib preview::server::port::tests::test_is_port_available_detects_ipv6_collision -- --ignored

        // Find a port that's free on BOTH stacks so we can isolate the
        // single-family-held case below. If we can't bind `::1` at all,
        // panicking here makes the cause obvious (preferable to silent skip).
        let port = (53500..53600)
            .find(|&candidate| {
                let v4 = std::net::TcpListener::bind(("127.0.0.1", candidate));
                let v6 = std::net::TcpListener::bind(("::1", candidate));
                v4.is_ok() && v6.is_ok()
            })
            .expect(
                "no port in 53500..53600 was bindable on both IPv4 and IPv6; \
                 IPv6 loopback may be disabled on this host (re-run without --ignored)",
            );

        // Hold ONLY IPv6 [::1]:port — IPv4 is intentionally left free.
        // This mimics the eleventy `*:8080` (IPv6 wildcard) scenario.
        let _v6_holder = std::net::TcpListener::bind(("::1", port))
            .expect("Should bind IPv6 loopback for the test");

        // With dual-stack-aware is_port_available, this MUST return false
        // even though 127.0.0.1:port is free. Before the fix it returned true.
        assert!(
            !is_port_available(port),
            "is_port_available({}) must return false when IPv6 [::1]:{} is held, \
             even if IPv4 is free (dual-stack collision detection)",
            port, port
        );
    }

    #[tokio::test]
    async fn test_verify_server_ready_success() {
        // Test that verify_server_ready succeeds when a moss preview server
        // (one that returns the MOSS_HEALTH_MARKER on /__moss_health/) is running.
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;

        // Find an available port in a higher range to avoid conflicts
        let port = find_available_port(54000).expect("Should find available port");

        // Start a simple TCP listener on a thread (simulates a running server)
        let listener =
            TcpListener::bind(format!("127.0.0.1:{}", port)).expect("Should bind to port");

        // Set non-blocking so we can check the shutdown flag
        listener
            .set_nonblocking(true)
            .expect("set_nonblocking failed");

        // Shared flag to signal server thread to stop
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        // Spawn a thread to accept connections and send HTTP responses
        // containing the moss health marker.
        let handle = thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);
                        // Body that contains MOSS_HEALTH_MARKER ("moss-preview-server")
                        let body = "{\"server\":\"moss-preview-server\",\"version\":\"test\"}";
                        let response = format!(
                            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        // Give the listener time to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // verify_server_ready should succeed
        let result = verify_server_ready(port).await;

        // Signal shutdown and wait for thread
        shutdown.store(true, Ordering::Relaxed);
        let _ = handle.join();

        assert!(
            result.is_ok(),
            "verify_server_ready should succeed for moss server with health marker: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_verify_server_ready_rejects_foreign_server() {
        // Regression for the 2026-05-22 blind-reuse bug:
        // a non-moss server returning HTTP 200 on / (eleventy, vite, etc.)
        // must NOT be accepted as a moss server. The old verify_server_ready
        // accepted any 200/4xx/5xx as "server is up"; the new check requires
        // the MOSS_HEALTH_MARKER in the response body.
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;

        let port = find_available_port(54500).expect("Should find available port");

        let listener =
            TcpListener::bind(format!("127.0.0.1:{}", port)).expect("Should bind to port");
        listener.set_nonblocking(true).expect("set_nonblocking failed");

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        // This thread simulates an eleventy server: returns 200 OK with a
        // generic HTML body that does NOT contain the moss health marker.
        let handle = thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);
                        // Foreign server response: 200 OK, plausible body,
                        // NO moss marker.
                        let body = "<!DOCTYPE html><html><body>Hello from Eleventy</body></html>";
                        let response = format!(
                            "HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let result = verify_server_ready(port).await;

        shutdown.store(true, Ordering::Relaxed);
        let _ = handle.join();

        assert!(
            result.is_err(),
            "verify_server_ready must reject a foreign server that lacks the moss marker"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not a moss") || msg.contains("foreign"),
            "Error should explain why: got {:?}",
            msg
        );
    }

    #[tokio::test]
    async fn test_verify_server_ready_fails_on_closed_port() {
        // Use a port that is not in use (high port number)
        // verify_server_ready should fail after max attempts
        let result = verify_server_ready(59999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to respond"));
    }
}
