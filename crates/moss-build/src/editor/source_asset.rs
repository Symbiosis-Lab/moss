//! Project-scoped SOURCE file bytes for the editor — the carrier-free core.
//!
//! The editor renders the source the user typed, pre-transformation: this path
//! does NOT go through the build's AssetRegistry, and never serves anything out
//! of `.moss/build` (ADR-022).
//!
//! [`serve_source_asset`] is the whole behaviour. Two carriers wrap it:
//!
//! - the `moss-source://` Tauri scheme (`source_asset_protocol::register`),
//!   used by the desktop app;
//! - `GET /__moss/source/*path` on the preview server
//!   (`preview::server::router::source_asset_route`) — used by any host with no
//!   custom-scheme support, i.e. a browser or an Obsidian pane.
//!
//! They are adapters, not implementations: each resolves a project root its own
//! way and dresses the shared answer in its own response type. Anything that
//! decides WHAT to serve belongs here, so the two carriers cannot drift.
//! [`resolve_scoped`] is the containment boundary for both — no caller may
//! widen it. Nothing in this file names a `tauri::` type; the scheme mount
//! stays in the `source_asset_protocol` sibling (the E2 split line).

use std::path::{Path, PathBuf};

/// Resolve a request path (the URL path of `moss-source://localhost/<p>`,
/// percent-DECODED, leading '/' allowed) to an absolute source file path,
/// rejecting anything that escapes `project_root`. Returns None on escape or
/// when the resolved path is not an existing file.
pub fn resolve_scoped(project_root: &Path, request_path: &str) -> Option<PathBuf> {
    let rel = request_path.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }
    let canonical_root = std::fs::canonicalize(project_root).ok()?;
    let joined = canonical_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let canonical = std::fs::canonicalize(&joined).ok()?;
    if !canonical.starts_with(&canonical_root) {
        return None; // escaped the project root
    }
    if !canonical.is_file() {
        return None;
    }
    // Staying inside the vault is not enough: the vault CONTAINS secrets.
    // `.moss/identity-key` is the user's private signing key and `.moss/config.toml`
    // can hold an api_key, so "scoped to the project" would still serve them. That
    // was survivable while the only caller was moss's own webview; it is not once
    // the same resolver answers on a TCP port every local process can reach.
    //
    // Deny any dot-prefixed component, the rule every general-purpose web server
    // uses. It covers `.moss/`, and also `.git/`, `.env` and `.ssh` if a vault ever
    // contains them. One carve-out: `.moss/theme/` is the site's design assets,
    // served verbatim on the published site at `/_moss/theme/`, so there is no
    // secret to keep and the editor's viewer needs them (a theme image otherwise
    // listed in the tree but 404 in the viewer). The dot rule still applies to
    // components below `theme/`.
    let rel = canonical.strip_prefix(&canonical_root).ok()?;
    // On a miss `rel` still starts with `.moss`, which the dot rule then denies.
    let inside = rel.strip_prefix(".moss/theme").unwrap_or(rel);
    let denied_dotfile = inside.components().any(|c| {
        matches!(c, std::path::Component::Normal(name)
            if name.to_string_lossy().starts_with('.'))
    });
    if denied_dotfile {
        return None;
    }
    Some(canonical)
}

/// What a source-asset request resolved to, independent of how it arrived.
///
/// Deliberately not a `tauri::http::Response` or an `axum::Response`: this is
/// the answer, and each mount dresses it in its own carrier's type. Keeping it
/// carrier-free is what lets the two mounts share one implementation instead of
/// being two implementations held in sync by a test.
pub struct SourceAssetResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl SourceAssetResponse {
    pub fn not_found() -> Self {
        Self {
            status: 404,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: b"Not Found".to_vec(),
        }
    }
}

/// Resolve and read a project-scoped SOURCE file — the whole behaviour behind
/// both mounts (the `moss-source://` Tauri scheme and the preview server's
/// `/__moss/source/` route).
///
/// `request_path` must already be percent-DECODED. Escapes outside
/// `project_root` and missing files are 404 by way of [`resolve_scoped`], which
/// is the security boundary; nothing here may widen it.
pub async fn serve_source_asset(project_root: &Path, request_path: &str) -> SourceAssetResponse {
    let Some(file) = resolve_scoped(project_root, request_path) else {
        return SourceAssetResponse::not_found();
    };
    match read_source_asset(file.clone()).await {
        Ok(body) => SourceAssetResponse {
            status: 200,
            content_type: crate::editor::resolve::asset_resolver::mime_for_path(&file).to_string(),
            body,
        },
        Err(_) => SourceAssetResponse::not_found(),
    }
}

/// Reads a source asset off the async runtime, waiting briefly for a
/// cloud-evicted file to come back.
///
/// `tokio::task::spawn_blocking`, not `tauri::async_runtime::spawn_blocking`:
/// both mounts already run inside a tokio runtime (the axum handler natively;
/// the Tauri scheme via `async_runtime::spawn`, which drives Tauri's own tokio
/// runtime), and this file is the tauri-free half of the split.
async fn read_source_asset(file: std::path::PathBuf) -> std::io::Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        crate::build::cloud_readiness::read_with_materialize_wait(
            &file,
            crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
        )
    })
    .await
    .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_nested_source_file_and_rejects_escape() {
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        fs::create_dir_all(p.join("图片/摄影")).unwrap();
        fs::write(p.join("图片/摄影/DSCF4053.jpeg"), b"jpegbytes").unwrap();

        // In-root nested path resolves.
        let got = resolve_scoped(p, "/图片/摄影/DSCF4053.jpeg").unwrap();
        assert!(got.ends_with("图片/摄影/DSCF4053.jpeg"));

        // Escape attempt is rejected.
        assert!(resolve_scoped(p, "/../../etc/passwd").is_none());
        // Missing file is rejected.
        assert!(resolve_scoped(p, "/nope.jpg").is_none());
    }

    /// Being inside the vault is not enough to be servable.
    ///
    /// `.moss/identity-key` is the user's private signing key and it lives INSIDE
    /// the vault, so the containment check alone happily resolves it. That was
    /// tolerable while only moss's own webview could ask; it is a key disclosure
    /// once the same resolver answers on a TCP port. Each assertion below is a
    /// real file this would have served.
    #[test]
    fn secrets_inside_the_vault_are_refused_even_though_they_are_contained() {
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        fs::create_dir_all(p.join(".moss")).unwrap();
        fs::write(p.join(".moss/identity-key"), b"DEADBEEF").unwrap();
        fs::write(p.join(".moss/config.toml"), b"api_key = \"sk-live\"").unwrap();
        fs::create_dir_all(p.join(".git")).unwrap();
        fs::write(p.join(".git/config"), b"[remote]").unwrap();
        fs::write(p.join(".env"), b"TOKEN=1").unwrap();

        // Sanity: these files really exist and really are inside the vault, so a
        // None here means the dotfile rule refused them — not that they were missing.
        assert!(p.join(".moss/identity-key").is_file());

        assert!(resolve_scoped(p, "/.moss/identity-key").is_none(), "private key");
        assert!(resolve_scoped(p, "/.moss/config.toml").is_none(), "config w/ api_key");
        assert!(resolve_scoped(p, "/.git/config").is_none(), "git config");
        assert!(resolve_scoped(p, "/.env").is_none(), "dotfile at the root");

        // Theme assets are public on the built site (`/_moss/theme/`), so the
        // fence lets them through — that is what the editor's viewer loads.
        fs::create_dir_all(p.join(".moss/theme")).unwrap();
        fs::write(p.join(".moss/theme/grain.png"), b"png").unwrap();
        fs::write(p.join(".moss/theme/.secret"), b"x").unwrap();
        assert!(resolve_scoped(p, "/.moss/theme/grain.png").is_some(), "theme asset served");
        assert!(resolve_scoped(p, "/.moss/theme/.secret").is_none(), "dotfile under theme");

        // And the rule must not swallow ordinary content.
        fs::write(p.join("photo.jpg"), b"jpeg").unwrap();
        assert!(resolve_scoped(p, "/photo.jpg").is_some(), "normal file still served");
    }

    /// The tree's `.moss/` allowlist and this fence were once written apart, and
    /// a theme image listed in the tree 404'd in the viewer. Walk the allowlist:
    /// the theme is the one entry whose files the viewer loads as assets and it
    /// must resolve; the rest are opened as text through `read_file`, never as
    /// an asset URL, and must stay refused (`config.toml` can hold secrets).
    #[test]
    fn tree_allowlist_and_fence_agree() {
        use crate::build::scan::classify::MOSS_INTERNAL_ALLOWLIST;
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        for allowed in MOSS_INTERNAL_ALLOWLIST {
            let probe = if allowed.ends_with(".toml") {
                fs::create_dir_all(p.join(".moss")).unwrap();
                allowed.to_string()
            } else {
                fs::create_dir_all(p.join(allowed)).unwrap();
                format!("{allowed}/probe.png")
            };
            fs::write(p.join(&probe), b"x").unwrap();
            let served = resolve_scoped(p, &format!("/{probe}")).is_some();
            assert_eq!(served, allowed.starts_with(".moss/theme"), "{probe}");
        }
    }
}
