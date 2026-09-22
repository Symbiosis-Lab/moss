//! The carrier's session: which vault the HTTP command carrier is serving and
//! the token that unlocks it, as ONE record (ADR-075 rule 4). `invoke.rs`
//! holds the arms; this file holds what they run against.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio_util::sync::CancellationToken;

use crate::types::runtime::ServerHandle;
use crate::vault::paths::VaultRoot;

/// The carrier's binding cell — the analogue of
/// `plugins::engine::host_fns::HostState`, which builds the app's managed
/// singletons for the plugin-engine carrier. It holds the [`Session`] the
/// server currently serves: the vault root the path-taking arms resolve
/// against (the same root the Tauri command bodies read via
/// `state.require_project_path()`) and the token that unlocks it.
///
/// The session is derived per-request from the server's live `site_dir`
/// pointer (which stays correct across folder switches) — see [`Self::bind`]. The token and the vault are ONE record: a token is
/// minted when a vault is bound and retired when a different vault is bound,
/// so a forgotten browser tab cannot follow the server to whatever folder it
/// is next pointed at (ADR-075 rule 4). Before this, the token was minted once
/// per server while the root moved underneath it per request.
///
/// A request never reads this cell twice. The token gate binds, admits, and
/// hands the handler the very [`Session`] it admitted (as a request
/// extension), so a rebind that lands mid-request cannot make a request
/// admitted under one vault execute against another.
#[derive(Clone)]
pub struct InvokeCtx {
    session: Arc<RwLock<Option<Arc<Session>>>>,
}

/// The vault a carrier is currently serving and the token that unlocks it.
/// Replaced whole on a folder switch — never one field without the other.
/// Every arm runs against the session its request was admitted under.
pub struct Session {
    vault: VaultRoot,
    /// `Arc` so the token gate can hold the current secret across an await
    /// without cloning the bytes. See [`super::carrier_token`].
    token: Arc<String>,
    /// Fires when this session is replaced. The SSE stream ends on it, so a
    /// subscriber admitted under this vault stops receiving the next vault's
    /// events the moment the switch is observed.
    retired: CancellationToken,
}

impl Session {
    pub fn vault(&self) -> &VaultRoot {
        &self.vault
    }

    pub fn token(&self) -> &Arc<String> {
        &self.token
    }

    /// Resolves when a different vault has been bound over this session.
    pub fn retired(&self) -> &CancellationToken {
        &self.retired
    }
}

impl InvokeCtx {
    /// A carrier context with no Tauri shell (CLI / headless `moss build
    /// --serve`), and the GUI's too — the router binds from `site_dir` at
    /// start-up and every request re-derives, so no host seeds anything.
    /// Serves no vault and holds no token until [`Self::bind`] runs.
    pub fn standalone() -> Self {
        Self {
            session: Arc::new(RwLock::new(None)),
        }
    }

    /// The current session token, `None` while no vault is bound. Read by a
    /// host's own test to authenticate after `start_server` has bound; the
    /// token gate reads the session instead. Never logged.
    pub fn token(&self) -> Option<Arc<String>> {
        self.session
            .read()
            .ok()
            .and_then(|s| s.as_ref().map(|s| s.token.clone()))
    }

    /// Bind the carrier to the vault the live serve dir sits in and return the
    /// session now in force — authoritative across folder switches. Binding
    /// the vault already bound returns the existing session; binding a
    /// different one mints a fresh token, publishes it to that vault's
    /// `.moss/build.nosync/http-token`, and retires the previous session — which is
    /// what invalidates every token handed out for the previous vault. A serve
    /// dir that resolves to no vault retires the session in force and returns
    /// `None`: the gated tiers then 401 and the token-free tier reports no
    /// project open, so a token minted for the last vault cannot outlive it.
    ///
    /// Called by the token gate BEFORE it compares the presented token, by the
    /// token-free handler, and by [`ServerHandle::point_at`] the moment the
    /// app switches folders. Each caller then runs against the session this
    /// returned, never against a second read of the cell — so a request that
    /// crosses a folder switch is judged against, and executed in, one vault.
    ///
    /// The serve dir is read, the vault resolved, and mint, publish and store
    /// all happen under the one write lock deliberately: two requests racing
    /// across a switch must not both mint, or the file on disk could disagree
    /// with the record, and a request that read the serve dir before a switch
    /// must not rebind the old vault over the new one afterwards. The publish
    /// is a small synchronous write that runs only on a switch; do not move
    /// it off the lock. Best-effort on the publish itself: a vault whose build
    /// dir cannot be written still gets a token, and a client that cannot read
    /// it is simply 401 — the read-only tier is unaffected.
    pub fn bind(&self, site_dir: &Arc<RwLock<PathBuf>>) -> Option<Arc<Session>> {
        let mut session = self.session.write().unwrap_or_else(|e| e.into_inner());
        let vault = site_dir
            .read()
            .ok()
            .and_then(|dir| VaultRoot::find_containing(&dir));
        let Some(vault) = vault else {
            if let Some(previous) = session.take() {
                previous.retired.cancel();
            }
            return None;
        };
        if let Some(current) = session.as_ref().filter(|s| s.vault == vault) {
            return Some(current.clone());
        }
        let token = Arc::new(super::carrier_token::mint());
        match super::carrier_token::publish(vault.path(), &token) {
            Ok(()) => log::info!(
                target: "preview",
                "HTTP carrier token published under .moss/build.nosync/ (loopback-readable)"
            ),
            Err(e) => log::warn!(target: "preview", "could not publish HTTP carrier token: {e}"),
        }
        let fresh = Arc::new(Session {
            vault,
            token,
            retired: CancellationToken::new(),
        });
        if let Some(previous) = session.replace(fresh.clone()) {
            previous.retired.cancel();
        }
        Some(fresh)
    }
}

impl ServerHandle {
    /// Point the server at a new serve dir — the folder switch. Rebinds the
    /// carrier in the same step, so the previous vault's token and SSE
    /// streams are retired when the switch happens, not when the next
    /// carrier request happens to observe it (the desktop app talks Tauri
    /// IPC, so without this nothing guarantees such a request ever comes).
    pub fn point_at(&self, dir: PathBuf) {
        if let Ok(mut current) = self.site_dir.write() {
            *current = dir;
        }
        if let Some(ctx) = &self.carrier {
            ctx.bind(&self.site_dir);
        }
    }
}
