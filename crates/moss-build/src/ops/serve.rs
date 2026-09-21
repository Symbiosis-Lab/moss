//! The preview server — one Axum server, two hosts (desktop GUI and headless
//! CLI), crossed from the app crate at S1 of the ADR-067 relocation.
//!
//! # The `/__moss/*` HTTP contract
//!
//! Every moss-internal endpoint lives under the `/__moss/` (and `/__moss_health/`)
//! namespace, and every one of them MUST be registered as an explicit `.route(...)`
//! entry BEFORE the router's `.fallback(ServeDir)` — that ordering is what stops a
//! user file at the same path from shadowing a moss endpoint. Add future internal
//! endpoints under this prefix, following the same ordering rule.
//!
//! ## The three-tier token model
//!
//! The command carrier splits one command registry across three routes, one per
//! trust tier. The tier is in the URL path, so it is structural rather than a
//! runtime flag, and the three allowlists are disjoint (`invoke.rs`):
//!
//! * `POST /__moss/invoke/<cmd>` — **token-free read**. Public-read commands
//!   only, exactly like `/__moss/source/*`. Its one path-taking arm confines the
//!   path to the vault; that confinement is the whole defence on this tier.
//! * `POST /__moss/read/<cmd>` — **token-gated read**. The editor's boot + open
//!   reads: booting the editor exposes the whole vault's tree, so it is a
//!   session-scoped capability, not a public read.
//! * `POST /__moss/mutate/<cmd>` — **token-gated mutation**. Confined writes
//!   into the vault, nothing else.
//! * `GET /__moss/events` — **token-gated SSE stream** of the same typed
//!   `MossEvent` bus the desktop frontend receives over Tauri IPC (`events.rs`).
//!   Same token gate as `/read`, minus the media-type half (a GET has no body).
//!
//! ## The token
//!
//! One token per bound vault, minted by `InvokeCtx::bind` — at start-up from
//! the serve dir, and again whenever a folder switch points the server at a
//! different vault, which retires the previous token — and carried in the
//! `X-Moss-Token` request header. It is published — never logged — to
//! `.moss/build/http-token` inside the served vault (`carrier_token::publish`),
//! world-unreadable, so a local client (coding agent, Playwright) can
//! authenticate before its first request. A request with a bad or absent token
//! gets 401; a command absent from a tier's allowlist gets 404 (absent, not
//! gated — ADR-032 §5).
//!
//! ## Trust boundary
//!
//! The server binds loopback only (dual-stack `127.0.0.1` + `[::1]`), and the
//! OUTERMOST router layer (`trust_boundary::validate_host_origin`) refuses a
//! non-loopback `Host` (DNS-rebinding defence) and a foreign `Origin` with 403
//! before any handler runs — including on the token-gated tiers, so the token
//! is never even inspected for a foreign caller. Which carrier a given moss
//! instance mounts is governed by ADR-066 (one carrier per instance): the
//! routes exist only when the host threads an `InvokeCtx` into [`ServeConfig`].
//!
//! No behavioral change crossed with the code: this module doc is the contract
//! statement the relocation plan's decision 7 called for, and a
//! `docs/reference/` page is owed when the open repo ships (decision #32's
//! publishing half), not before.
//!
//! ## Module layout
//! - `router` — Axum router construction, [`ServeConfig`]/[`start_server`], middleware stack
//! - `port` — port availability checking, scanning, readiness verification
//! - `invoke` — the HTTP command carrier (three tiers above)
//! - `carrier_token` — per-session token mint/publish + the gate middleware
//! - `trust_boundary` — Host/Origin validation (outermost layer)
//! - `events` — the SSE event carrier + headless announcer/reporter
//! - `placeholder` — SVG placeholders for assets still being processed
//! - `asset_rewriter` / `content_wrapper` / `iframe_bridge` / `comment_stub` —
//!   the response-transform layers (bridge injection stays a host-mounted layer
//!   per ADR-067 clause 4; today every host mounts all of them)

// `pub` only where a consumer outside this crate actually reads the module.
// `asset_rewriter` earns it (src-tauri/tests/instant_preview_media_parity_test.rs);
// the rest of the response-transform layers, the token and the trust boundary are
// internal to the server and stay `pub(crate)` — they are the security-relevant
// half, and the open repo will make "crate-external" mean "public".
pub mod asset_rewriter;
pub(crate) mod carrier_token;
pub(crate) mod comment_stub;
pub(crate) mod content_wrapper;
pub mod events;
pub(crate) mod iframe_bridge;
pub mod invoke;
pub mod placeholder;
pub mod port;
pub mod router;
pub mod session;
pub(crate) mod trust_boundary;

pub use invoke::InvokeCtx;
pub use router::{start_server, ServeConfig};

use std::sync::Arc;

/// Start a preview server with no host shell — the CLI / headless
/// `moss build --serve` arm. The GUI arm (server reuse, folder-switch
/// re-keying, managed state) is the app's `launch_server` impl and lives in
/// the app crate; this crate has exactly one construction path.
///
/// # Arguments
/// * `moss_path` — the project's `.moss` directory.
/// * `cli_site_dir` — the directory cell this server serves from. **The caller
///   must pass the same `Arc` the build moves**, or the server never sees a
///   render move it to staging and serves the pre-build seed for the life of the
///   process — a 404 on every page for `moss build --serve`. `None` means
///   "nobody is switching this", which is only true when no build shares the
///   process; the fallback cell is seeded to the initial serve dir
///   (current → staging → site).
///
/// Returns the bound port and the server's shutdown sender. The caller OWNS
/// the sender: dropping it puts the server into graceful-shutdown mode (it
/// stops accepting new connections while the OS backlog still completes TCP
/// handshakes — connections accepted at the TCP level that get no HTTP
/// response), so hold it for the life of the serve and `send(())` on the way
/// out. Until slice C1 this function `mem::forget`ed the sender because every
/// caller exited via `std::process::exit`; the CLI driver now blocks on
/// ctrl-C and shuts down deliberately (`ops::run_headless_build`), and the
/// one caller that still cannot send — an in-process test build — forgets it
/// at its own site (`events/host_ports.rs`, app crate).
pub async fn start_server_headless(
    moss_path: &str,
    cli_site_dir: Option<Arc<std::sync::RwLock<std::path::PathBuf>>>,
    asset_registry: Option<Arc<crate::types::assets::AssetRegistry>>,
) -> Result<(u16, tokio::sync::oneshot::Sender<()>), String> {
    let serve_dir = serve_dir_for_site_path(moss_path);
    let site_dir_state =
        cli_site_dir.unwrap_or_else(|| Arc::new(std::sync::RwLock::new(serve_dir)));
    // Headless mode: the HTTP command carrier is the entire point — typing
    // `moss build --serve` IS the opt-in (ADR-066). Standalone context, no
    // Tauri shell (mirrors host_fns::HostState::standalone).
    let invoke_ctx = Some(InvokeCtx::standalone());
    // The build's own registry, so a variant still being encoded answers with
    // the source bytes or a placeholder rather than 404. `<picture>` does not
    // recover from a chosen-source 404 (ADR-013), and until 2026-08-29 this
    // arm passed no registry at all — so the never-404 promise held in the app
    // and not under `moss build --serve` (#1113).
    let (port, shutdown_tx) = start_server(ServeConfig {
        invoke: invoke_ctx,
        asset_registry,
        ..ServeConfig::new(site_dir_state, port::env_port_base())
    })
    .await?;
    Ok((port, shutdown_tx))
}

/// Derive the cold-start serve dir for a folder from its `.moss` directory
/// path: the frozen `current` generation, else `/staging`. Mirrors
/// `build.rs`'s initial seed via `MossPaths::initial_serve_dir()`.
///
/// Also used by the app's folder-switch re-key, so a reused server rests on
/// the new folder's `current` generation rather than the old folder's.
pub fn serve_dir_for_site_path(moss_path: &str) -> std::path::PathBuf {
    crate::moss_paths::MossPaths::from_moss_dir(std::path::PathBuf::from(moss_path))
        .initial_serve_dir()
}

/// One WARN per 10 s per served root for preview 404s, with the count it held
/// back — so a pointer move that 404s a whole page load names itself once
/// instead of once per asset.
const PREVIEW_404_LOG_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

static PREVIEW_404_LOG: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, (Option<std::time::Instant>, usize)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Which tree a 404 came from: `staging`, `current→gen <id>`, or `other`.
fn served_tree_label(current_dir: &std::path::Path) -> String {
    match current_dir.file_name().and_then(|n| n.to_str()) {
        Some("staging") => "staging".to_string(),
        Some("current") => {
            let id = current_dir
                .parent()
                .map(|build| build.join("current.generation"))
                .and_then(|marker| std::fs::read_to_string(marker).ok())
                .map(|id| id.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!("current→gen {id}")
        }
        _ => "other".to_string(),
    }
}

pub(crate) fn log_preview_404(current_dir: &std::path::Path, path: &str) {
    let root = match current_dir.file_name().and_then(|n| n.to_str()) {
        Some("staging" | "current") => current_dir.parent().unwrap_or(current_dir),
        _ => current_dir,
    };
    let Ok(mut windows) = PREVIEW_404_LOG.lock() else { return };
    let now = std::time::Instant::now();
    let entry = windows.entry(root.to_path_buf()).or_insert((None, 0));
    if entry.0.is_some_and(|last| now.duration_since(last) < PREVIEW_404_LOG_WINDOW) {
        entry.1 += 1;
        return;
    }
    let suppressed = std::mem::take(&mut entry.1);
    entry.0 = Some(now);
    crate::build::lifecycle::root_identity::log_build_root(root, "serve", None);
    log::warn!(
        "preview 404 {} from {} ({} more suppressed)",
        path,
        served_tree_label(current_dir),
        suppressed
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_dir_for_site_path_resolves_current_for_folder_switch() {
        // Folder-switch re-key must point the reused server at the new folder's
        // frozen `current` generation, not at an empty directory (the
        // cold-start 404 regression, on the folder-switch path).
        use crate::moss_paths::MossPaths;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let proj = tempfile::TempDir::new_in(&base).expect("temp project");

        let mp = MossPaths::new(proj.path());
        mp.ensure_dirs().unwrap();
        std::fs::create_dir_all(mp.generation_dir("gen001")).unwrap();
        mp.set_current_ptr("gen001").unwrap();

        let moss_path = mp.root();
        let resolved = serve_dir_for_site_path(moss_path.to_str().unwrap());
        assert_eq!(
            resolved,
            mp.current_ptr(),
            "folder-switch re-key must rest on the frozen current generation, not empty /site"
        );
    }

}
