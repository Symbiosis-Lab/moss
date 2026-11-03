//! The HTTP command carrier: the engine's command table over
//! `POST /__moss/{invoke,read,mutate}/<cmd>`.
//!
//! A coding agent or a browser tab has no in-process route to the engine, so
//! the preview server already running for the folder carries the same typed
//! commands ([`crate::commands`]) over HTTP. This module adds nothing to them:
//! it maps a request to a tier, runs the command against the [`Session`] the
//! request was admitted under, and maps the outcome to a status.
//!
//! ## One tier per route
//!
//! * `/__moss/invoke/<cmd>`: [`commands::public_read::COMMANDS`], token-free like
//!   `/__moss/source/*`.
//! * `/__moss/read/<cmd>`: [`commands::session_read::COMMANDS`], behind the session
//!   token (`X-Moss-Token`), because they expose the whole folder.
//! * `/__moss/mutate/<cmd>`: [`commands::write::COMMANDS`], behind the same token.
//!
//! The tiers are disjoint, so the URL path alone fixes the trust class, and a
//! command outside a route's tier is absent (404), not refused.
//!
//! ## Trust boundary
//!
//! Every route sits behind `trust_boundary::validate_host_origin` and the
//! loopback bind; the token gate (`super::carrier_token`) runs after it on the
//! two gated routes. An HTTP body is attacker-controlled where moss's own UI is
//! not, so [`Session`]'s [`crate::commands::CommandHost::resolve`] confines every caller path to
//! the folder; the commands themselves cannot reach a path any other way.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::commands::{self, CommandError, CommandResult};

pub use super::session::{InvokeCtx, Session};

fn outcome_to_response(cmd: &str, outcome: Option<CommandResult>) -> Response {
    match outcome {
        None => (StatusCode::NOT_FOUND, format!("unknown or unexposed command: {cmd}")).into_response(),
        Some(Ok(value)) => (StatusCode::OK, Json(value)).into_response(),
        Some(Err(CommandError::BadArgs(msg))) => (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response(),
        Some(Err(CommandError::Failed(msg))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// `POST /__moss/invoke/*cmd`. Token-free, so nothing has bound a session yet:
/// it binds from the live serve dir, and a serve dir outside any folder answers
/// every public read with [`NO_PROJECT_OPEN`].
///
/// [`NO_PROJECT_OPEN`]: crate::vault::paths::NO_PROJECT_OPEN
pub async fn handle_invoke(ctx: InvokeCtx, site_dir: Arc<RwLock<PathBuf>>, cmd: String, args: Value) -> Response {
    let outcome = match ctx.bind(&site_dir) {
        Some(session) => commands::public_read::dispatch(&*session, &cmd, args).await,
        None if commands::public_read::COMMANDS.contains(&cmd.as_str()) => {
            Some(Err(CommandError::Failed(crate::vault::paths::NO_PROJECT_OPEN.to_string())))
        }
        None => None,
    };
    outcome_to_response(&cmd, outcome)
}

/// `POST /__moss/mutate/*cmd`. The token gate (a `route_layer`, see `router.rs`)
/// has already admitted the request and handed down the session it admitted.
pub async fn handle_mutate(axum::Extension(session): axum::Extension<Arc<Session>>, cmd: String, args: Value) -> Response {
    let outcome = commands::write::dispatch(&*session, &cmd, args).await;
    outcome_to_response(&cmd, outcome)
}

/// `POST /__moss/read/*cmd`, behind the same gate as [`handle_mutate`].
pub async fn handle_authed_read(
    axum::Extension(session): axum::Extension<Arc<Session>>,
    cmd: String,
    args: Value,
) -> Response {
    let outcome = commands::session_read::dispatch(&*session, &cmd, args).await;
    outcome_to_response(&cmd, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandHost, VaultPath};

    /// A session over a scratch vault (a temp dir with a `.moss/`, bound the
    /// way the router binds: through a serve-dir cell). The `TempDir` rides
    /// along so the vault outlives the session.
    fn scratch() -> (tempfile::TempDir, Arc<Session>) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join(".moss")).expect("mark the vault");
        let cell = Arc::new(RwLock::new(dir.path().to_path_buf()));
        let session = InvokeCtx::standalone().bind(&cell).expect("a vault binds");
        (dir, session)
    }

    /// A folder-relative path, as the editor sends it, resolves under the
    /// root; escapes still fail after the join.
    #[test]
    fn resolve_accepts_project_relative_paths_and_still_rejects_escapes() {
        let confine = |s: &Session, p: &str| crate::commands::resolve(s, &VaultPath::from(p));
        let (dir, ctx) = scratch();
        std::fs::write(dir.path().join("note.md"), "x").expect("write");

        let ok = confine(&ctx, "note.md").expect("relative path confines");
        assert_eq!(
            ok.canonicalize().expect("canon"),
            dir.path().join("note.md").canonicalize().expect("canon")
        );
        assert!(confine(&ctx, "../outside.md").is_err());
        assert!(confine(&ctx, "/etc/passwd").is_err());
    }

    /// `delete_entry` is a write the carrier serves, and it refuses escapes and
    /// the folder itself. The success path would move a real file to the OS
    /// trash, so only the guards are exercised.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_entry_is_served_and_confined() {
        let (dir, ctx) = scratch();

        let refused = |v: Value| async {
            match commands::write::dispatch(&*ctx, "delete_entry", v).await {
                Some(Err(_)) => {}
                other => panic!("expected a refused dispatch, got {other:?}"),
            }
        };
        refused(json!({ "path": "../outside.md" })).await;
        refused(json!({ "path": "/etc/passwd" })).await;
        refused(json!({ "path": dir.path().to_string_lossy() })).await; // the root itself
    }

    /// `resolve_attachment_dir` answers the configured folder for a page, the
    /// default when none is configured, and refuses a page outside the vault.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resolve_attachment_dir_reads_config_and_confines() {
        let (dir, ctx) = scratch();
        let ask = |p: &str| commands::session_read::dispatch(&*ctx, "resolve_attachment_dir", json!({ "pageRelativePath": p }));

        match ask("posts/a.md").await {
            Some(Ok(v)) => assert_eq!(v, json!("posts"), "default is beside the page"),
            other => panic!("expected the default dir, got {other:?}"),
        }

        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        std::fs::write(
            dir.path().join(".moss/config.toml"),
            "[editor]\nattachment_folder = \"assets\"\n",
        )
        .unwrap();
        match ask("posts/a.md").await {
            Some(Ok(v)) => assert_eq!(v, json!("assets"), "bare value is root-relative"),
            other => panic!("expected the configured dir, got {other:?}"),
        }

        assert!(matches!(ask("../outside.md").await, Some(Err(_))));
        assert!(matches!(ask("/etc/passwd").await, Some(Err(_))));
    }

    /// `resolve_page_source` answers the root with the vault's home page, a
    /// lossy-slug URL with its source, and refuses a URL that climbs out.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resolve_page_source_resolves_home_and_confines() {
        let (dir, ctx) = scratch();
        std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\n---\nhi").unwrap();
        let ask = |u: &str| commands::session_read::dispatch(&*ctx, "resolve_page_source", json!({ "urlPath": u }));

        for url in ["", "/"] {
            match ask(url).await {
                Some(Ok(v)) => {
                    assert_eq!(v["is_dir"], json!(true), "{url:?}: {v}");
                    assert!(v["source_path"].as_str().is_some_and(|p| p.ends_with("index.md")), "{v}");
                }
                other => panic!("expected the home page, got {other:?}"),
            }
        }
        match ask("nothing/here/").await {
            Some(Ok(v)) => assert_eq!(v["source_path"], json!(null)),
            other => panic!("expected no source, got {other:?}"),
        }
        assert!(matches!(ask("../outside/").await, Some(Err(_))));
    }

    /// A command outside the tier dispatches to `None`, which the router turns
    /// into a 404 (absent, not refused).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unlisted_command_dispatches_to_none() {
        let (_dir, ctx) = scratch();
        assert!(
            commands::public_read::dispatch(&*ctx, "reveal_entry", json!({})).await.is_none(),
            "a command outside every tier is absent from this carrier"
        );
        assert!(
            commands::public_read::dispatch(&*ctx, "no_such_command_xyz", json!({})).await.is_none(),
        );
    }

    #[tokio::test]
    async fn cloud_unavailable_details_are_an_authenticated_session_read() {
        let (dir, ctx) = scratch();
        let args = json!({ "folderPath": dir.path().to_string_lossy() });
        let result = commands::session_read::dispatch(&*ctx, "get_cloud_unavailable_files", args)
            .await
            .expect("the command is on the authed-read carrier");
        let error = result.expect_err("a scratch vault has no active sweep");
        assert!(matches!(error, CommandError::Failed(message) if message == "No active folder sweep"));
    }

    /// A listed command answers with its real result.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_shortcodes_dispatches_and_returns_a_result() {
        let (_dir, ctx) = scratch();
        let out = commands::public_read::dispatch(&*ctx, "scan_shortcodes", json!({ "text": "# hi\n\nplain" }))
            .await
            .expect("listed command must dispatch")
            .expect("scan_shortcodes is infallible on valid text");
        // EditorScanResult is an object — proves we got a real serialized result.
        assert!(out.is_object(), "expected an EditorScanResult object, got: {out}");
    }

    /// `get_publish_preflight` answers with the session folder's own projection.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_publish_preflight_reads_the_session_folder() {
        let (dir, ctx) = scratch();
        let root = dir.path().to_string_lossy().to_string();
        crate::system::build_records::records().install_publish_preflight(
            &root,
            crate::build::types::PublishPreflightProjection { build_generation: 7, missing_references: vec![], unresolved_inputs: vec![], drafts_linked: vec![] },
        );

        let out = commands::session_read::dispatch(&*ctx, "get_publish_preflight", json!({ "folderPath": root }))
            .await
            .expect("listed command must dispatch")
            .expect("a recorded projection is Ok");
        assert_eq!(out["build_generation"], json!(7));
    }

    /// A folder this process has not built yet answers `null`: no verdict.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_publish_preflight_answers_null_for_an_unbuilt_vault() {
        let (dir, ctx) = scratch();
        let out = commands::session_read::dispatch(&*ctx, "get_publish_preflight", json!({ "folderPath": dir.path() }))
            .await
            .expect("listed command must dispatch")
            .expect("no build recorded is still Ok(None)");
        assert!(out.is_null(), "expected null, got: {out}");
    }

    /// `list_vault_terms` groups the terms in the folder's article map by kind.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_vault_terms_groups_the_article_map_terms() {
        let (dir, ctx) = scratch();
        std::fs::create_dir_all(dir.path().join(".moss").join("build.nosync")).expect("build.nosync");

        let mut map = crate::build::scan::article_map::ArticleMap::new();
        map.kinds = vec![crate::build::terms::TermKind {
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false,
            parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        map.terms.insert(
            "people/scarly".to_string(),
            crate::build::terms::TermSite { display: "Scarly".to_string(), claimed_by: None, parent: None },
        );
        map.save(&dir.path().join(".moss")).expect("save fixture");

        let out = commands::session_read::dispatch(&*ctx, "list_vault_terms", json!({}))
            .await
            .expect("listed command must dispatch")
            .expect("a valid article map is Ok");
        assert_eq!(out["kinds"]["people"]["names"], json!(["Scarly"]));
    }

    /// Bad args surface as `BadArgs`, mapped to 400 by the handler.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_required_arg_is_bad_args() {
        let (_dir, ctx) = scratch();
        let r = commands::public_read::dispatch(&*ctx, "scan_shortcodes", json!({}))
            .await
            .expect("listed")
            .expect_err("missing 'text' must be an error");
        assert!(matches!(r, CommandError::BadArgs(_)), "missing arg must be BadArgs (→400)");
    }
}
