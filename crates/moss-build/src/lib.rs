//! moss-build — the UI-free build pipeline crate.
//!
//! Seeded 2026-08-11 with vault-root identity per
//! [ADR-051](../../../../docs/decisions/ADR-051-vault-root-ships-in-moss-build.md): root
//! identity is the compiler's *input identity* (`PipelineConfig.root` is a
//! [`vault_root::VaultRoot`]), so the owner lands here before the M6a pipeline move —
//! the moved pipeline files then reach it in-crate instead of needing a forbidden
//! `moss-build → moss` edge (`scripts/check-crate-dag.mjs` rule 4).
//!
//! Joined 2026-08-18 by the `.moss` **layout** owner per
//! [ADR-057](../../../../docs/decisions/ADR-057-moss-paths-ships-in-moss-build.md): the crate
//! that writes `.moss/build/generations/<id>/` owns the names for it, and 22 of
//! `MossPaths`'s consumers sit in the build tree that moves at M6a. Same argument as
//! ADR-051, second type — identity, then layout.
//!
//! Joined 2026-08-18 by [`config`] — the `.moss/config.toml` **reader** and the pure
//! migration transform, per
//! [ADR-059](../../../../docs/decisions/ADR-059-config-reader-and-migration-runner-after-the-crate-split.md).
//! The app still owns the file. Its modal-driven writers stay app-side; the one write
//! primitive they share (`vault::config`, amendment of 2026-09-07) is here so the open
//! binary's `env` write and the compiler's plugin uninstall edit the user's bytes
//! through the same door, never re-emit them.
//!
//! Until M6a the rest of the compiler charter
//! (scan → render → emit, headless, no tauri) migrates in per
//! `docs/reference/target/03-module-tree.md`.

pub mod advisory;
pub mod config;
pub mod moss_paths;
pub mod nested_roots;
pub mod vault_root;

// ── The M6a move (2026-08-28): the compiler itself ──────────────────────────
//
// The build tree and its private support cluster crossed from `src-tauri` in
// one enlarged `git mv` (docs/archive/2026-08-27-m6a-execution-plan.md § M).
// Files kept their intra-tree `crate::…` spellings, so the module names here
// mirror the app crate's: `build`, `i18n`, `types`, `tasks`, and the partial
// families below. `src-tauri` re-exports each at its old path (shim table:
// docs/reference/target/MIGRATION-STATE.md).
pub mod build;
// The editor backend (ADR-071): resolve/tree/content/source-asset cores the
// HTTP command carrier calls; the app keeps the Tauri wrappers + shims.
pub mod editor;
// The QuickJS plugin engine core (open-CLI slice 2, #1019): pure rquickjs +
// tokio, app reach injected through `engine::app_host::AppHost`.
pub mod engine;
pub mod i18n;
// Operational modes: the preview server (ops/serve, S1 of the ADR-067
// relocation, 2026-08-28); ops/watch follows at W1.
pub mod ops;
pub mod tasks;
pub mod types;

/// Partial families: only the members the build tree reaches crossed; the rest
/// of each family stays app-side and re-exports the crossed members.
pub mod cli;
pub mod deploy;
pub mod identity;
pub mod infra;
pub mod platform;
pub mod seta;
pub mod plugins;
pub mod system;

pub mod vault;

/// Crate-wide mutex serializing every test that mutates process-global env vars
/// (`MOSS_ENV`, `MOSS_SETA_URL`, `STRIPE_PUBLISHABLE_KEY`, `MOSS_MATTERS_*`).
/// Module-local mutexes don't serialize across modules in the same test binary.
/// Crossed with the build tree; `src-tauri` keeps its own copy — the lock is
/// per test binary, so a cross-crate re-export would serialize nothing.
#[cfg(test)]
pub(crate) static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read one raw HTTP/1.1 request off `stream` — headers through the blank
/// line, then exactly `Content-Length` more body bytes (0 if absent or
/// unparsable) — without answering it. Returns the raw request bytes read,
/// for a caller that needs to inspect what was sent (e.g. a header value).
///
/// Split from the answer ([`test_respond_and_close`]) so a mock that must act
/// *between* receiving a request and answering it — rewrite a file mid-deploy,
/// stall the first attempt — can. A mock that has nothing to do in between
/// calls [`test_mock_http_conn`], which is just the two in a row.
///
/// Shared because independent copies of this exact drain loop kept
/// accumulating — `seta::chunked_upload_tests`, `deploy::push::tests`,
/// `deploy::upload::tests`, `deploy::prebuilt_tests`, `seta::sites_tests` —
/// and a Content-Length parsing drift between them would have been silent.
/// Crate-wide like [`ENV_TEST_MUTEX`] above, for the same reason: the raw-TCP
/// mock pattern is used from test modules in different top-level families
/// (`seta`, `deploy`) with no natural single owner among them.
#[cfg(test)]
pub(crate) async fn test_drain_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(hdr_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let hdr_str = String::from_utf8_lossy(&raw[..hdr_end]);
            let body_len = hdr_str
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let expected_total = hdr_end + 4 + body_len;
            while raw.len() < expected_total {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            break;
        }
    }
    raw
}

/// Write `resp` and close the connection, telling the client it is closing.
///
/// A canned response that does not say `Connection: close` is sent with that
/// header added. The mock closes the socket right after one response, but a
/// response without the header tells the client's connection pool the socket
/// is reusable: the pool then sends the *next* request on a socket that is
/// already dead, and hyper reports the failed write as `IncompleteMessage`.
/// Whether it does depends on whether the FIN has reached the client yet,
/// which is kernel timing: about 1 run in 200 of a four-request mock with 16
/// test threads at once, almost never alone. So the header is added here, once,
/// rather than left to each caller's byte string to remember.
#[cfg(test)]
pub(crate) async fn test_respond_and_close(stream: &mut tokio::net::TcpStream, resp: &[u8]) {
    use tokio::io::AsyncWriteExt;
    stream.write_all(&announcing_close(resp)).await.ok();
    stream.shutdown().await.ok();
}

/// `resp` with `Connection: close` inserted after the status line, unless its
/// head already carries a `Connection` header (the caller's word stands).
#[cfg(test)]
fn announcing_close(resp: &[u8]) -> Vec<u8> {
    let head_end = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(resp.len());
    let says_connection = String::from_utf8_lossy(&resp[..head_end])
        .lines()
        .any(|l| l.to_ascii_lowercase().starts_with("connection:"));
    match resp.windows(2).position(|w| w == b"\r\n") {
        Some(status_end) if !says_connection => {
            let (status_line, rest) = resp.split_at(status_end + 2);
            [status_line, b"Connection: close\r\n", rest].concat()
        }
        _ => resp.to_vec(),
    }
}

/// Drain one request off `stream` ([`test_drain_http_request`]), answer it
/// with `resp` and close ([`test_respond_and_close`]). Returns the raw request
/// bytes read.
#[cfg(test)]
pub(crate) async fn test_mock_http_conn(
    mut stream: tokio::net::TcpStream,
    resp: &[u8],
) -> Vec<u8> {
    let raw = test_drain_http_request(&mut stream).await;
    test_respond_and_close(&mut stream, resp).await;
    raw
}

#[cfg(test)]
mod mock_http_tests;

/// Wine skip guard (ADR-033 amendment): a test may skip under Wine ONLY via
/// this self-detection — the `HKLM\Software\Wine` registry key exists in every
/// Wine prefix and is structurally absent on real Windows, so a skip taken
/// here can never hide a real-Windows failure. Callers must eprintln! the
/// skip so it stays loud in the run log.
#[cfg(all(test, windows))]
pub(crate) fn running_under_wine() -> bool {
    std::process::Command::new("reg")
        .args(["query", r"HKLM\Software\Wine"])
        .output()
        .is_ok_and(|o| o.status.success())
}
