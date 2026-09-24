//! Read-only HTTP command carrier — `POST /__moss/invoke/<cmd>`.
//!
//! ## Why this exists
//!
//! moss's commands normally reach Rust through the Tauri IPC seam (the desktop
//! webview calls `invoke("cmd", args)`). That seam needs the Tauri shell. A
//! coding agent or a Playwright script driving a headless preview has no shell —
//! so a *second carrier* mounts a read-only subset of the SAME commands over
//! HTTP, on the preview server that is already running. One registry, two
//! carriers — the same shape the `/__moss/source/*path` route already has, where
//! the HTTP mount calls the SAME `serve_source_asset` core the `moss-source://`
//! Tauri scheme uses (see `router.rs`).
//!
//! ## One list, second carrier — how the "no second surface" rule holds
//!
//! The ratchet row `command_surface_parity` (s) forbids a `#[tauri::command]`
//! that the ONE `command_list!` registry (`registry.rs`) cannot see. This module
//! declares NO commands. It only *dispatches* to command logic that is already
//! in that registry:
//!
//!   * the pure-args command (`scan_shortcodes`) is called directly — the arm
//!     IS the command body.
//!   * `State<'_, AppState>` commands (`editor_resolve_asset`,
//!     `editor_resolve_references`) cannot be called directly (`tauri::State`
//!     has a private field and is unconstructible outside Tauri), so the arm
//!     calls the SAME pure core the command body calls (`resolve_asset`,
//!     `resolve_references_batch`) with the project root the command would have
//!     read from `AppState`. This is the identical "two carriers, one core"
//!     seam `/__moss/source/` uses.
//!
//! The read-only *allowlist* — [`READ_ONLY_HTTP_COMMANDS`] — is the ONE manual
//! surface. It is a strict subset of `command_list!`, and the test
//! `allowlist_is_a_subset_of_the_command_registry` reads `registry.rs` and fails
//! if any allowlisted name is not a registered command. A command NOT in the
//! allowlist is **absent** from this carrier — the router returns 404, never a
//! runtime 403 (an unexposed capability is not gated, it does not
//! exist here).
//!
//! ## Three carriers on one seam, one per trust tier
//!
//! There are THREE routes, one per trust tier, and the tier is in the URL path
//! so it is structural rather than a runtime flag:
//!
//!   * `POST /__moss/invoke/<cmd>` — the READ-ONLY carrier. Token-FREE,
//!     public-read, exactly like `/__moss/source/*`. Allowlist:
//!     [`READ_ONLY_HTTP_COMMANDS`].
//!   * `POST /__moss/read/<cmd>` — the AUTHED-READ carrier. Requires the
//!     per-session token, like `/mutate`. These are the editor's boot + open
//!     reads (`editor_bootstrap`, `list_directory`, …): booting the editor
//!     exposes the whole vault's tree, so it is a session-scoped capability, not
//!     a public read. Allowlist: [`AUTHED_READ_HTTP_COMMANDS`].
//!   * `POST /__moss/mutate/<cmd>` — the MUTATION carrier. Requires the
//!     per-session token in the `X-Moss-Token` header (see
//!     [`super::carrier_token`]). Allowlist: [`MUTATION_HTTP_COMMANDS`].
//!
//! The token-gated tiers (`/read`, `/mutate`) carry the SAME token middleware, so
//! neither can be invoked token-free; a public read is reachable ONLY on the
//! token-free `/invoke` path. The three allowlists are disjoint, so the URL path
//! alone fixes the tier. The desktop Tauri IPC path is untouched by all of this.
//!
//! ## Trust boundary
//!
//! Both routes inherit the preview server's shipped posture: they sit behind
//! `trust_boundary::validate_host_origin` (the OUTERMOST router layer, so it runs
//! first) and the loopback-only dual-stack bind. The mutation route adds a
//! second, inner gate — the token middleware (`route_layer`) — which runs AFTER
//! the trust boundary. So a foreign `Origin` is refused with 403 before the
//! token is even inspected, and only a same-origin/loopback request with a valid
//! token, POST method, and `application/json` body reaches a mutation arm. See
//! `super::carrier_token`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub use super::session::{InvokeCtx, Session};

/// Outcome of one dispatch arm. Distinguishes a caller error (bad/absent args →
/// 400) from a command failure (the command's own `Err(String)` → 500), so the
/// HTTP status carries the same information the Tauri IPC reject would.
#[derive(Debug)]
enum ArmError {
    BadArgs(String),
    Command(String),
}

type ArmResult = Result<Value, ArmError>;

fn to_value<T: serde::Serialize>(v: T) -> ArmResult {
    serde_json::to_value(v).map_err(|e| ArmError::Command(format!("serialize result: {e}")))
}

fn parse_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, ArmError> {
    serde_json::from_value(args).map_err(|e| ArmError::BadArgs(e.to_string()))
}

/// The project root the `State<'_, AppState>` commands would read — the vault
/// of the session this request was admitted under. Every arm runs against a
/// bound session; the no-vault case is answered once, in [`handle_invoke`],
/// before any arm is chosen.
fn project_root(session: &Session) -> PathBuf {
    session.vault().path().to_path_buf()
}

/// Confine a caller-supplied path to the project root — the carrier's trust
/// boundary.
///
/// **Every arm taking a path-shaped argument must route it through here.** The
/// Tauri IPC path does not need this: its arguments come from moss's own UI,
/// which only ever names files it opened from the vault. An HTTP body is
/// attacker-controlled, so the same command reached over the carrier is a
/// different trust proposition, and the check belongs at the boundary rather
/// than inside the shared core (confining the core would also re-confine the
/// desktop path, which legitimately reads outside the vault).
///
/// Reuses [`crate::vault::fs::validate_entry_path`] — the guard `create_files`
/// already goes through — and then performs the canonicalize-and-recheck that
/// helper's own doc prescribes for callers that need symlink-escape defence.
/// A path that does not exist yet is allowed through on the lexical check alone
/// (that is the rename/create case); one that does exist must still be inside
/// the root after resolving every symlink.
fn confine(ctx: &Session, user_path: &str) -> Result<PathBuf, ArmError> {
    let root = project_root(ctx);
    // A relative path is project-root-relative on BOTH carriers: the desktop
    // command bodies resolve `from_file` and friends against the open project,
    // and the shipping editor sends vault-relative paths on its boot path — so
    // the arm joins onto the root before the containment checks instead of
    // rejecting lexically. The `..` rejection in `validate_entry_path` still
    // sees the original segments because the join is textual, not normalized.
    let joined;
    let user_path = if Path::new(user_path).is_absolute() {
        user_path
    } else {
        joined = root.join(user_path).to_string_lossy().into_owned();
        &joined
    };
    let lexical =
        crate::vault::fs::validate_entry_path(&root, user_path).map_err(ArmError::Command)?;

    // Containment — including symlink-escape, for both an existing path and a
    // not-yet-created one — is `validate_entry_path`'s job now, so the arm has
    // no second rule of its own to keep in step. Resolving again here is only
    // to hand back the real path when there is one; a path that does not exist
    // yet has no resolved form and travels as written.
    Ok(lexical.canonicalize().unwrap_or(lexical))
}

// ── Dispatch arms ────────────────────────────────────────────────────────────
//
// Arg keys are camelCase, matching the exact wire shape Tauri IPC uses (see
// `bindings.ts`: `editor_resolve_asset` → `{ target, fromFile }`,
// `parse_frontmatter` → `{ filePath }`). An agent that already knows the IPC
// wire shape sends the identical body here.

/// `scan_shortcodes(text)` — pure; the arm IS the command body.
async fn arm_scan_shortcodes(_ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        text: String,
    }
    let a: A = parse_args(args)?;
    // The command wrapper is a pure delegation to `moss_core::ast::editor_scan`
    // (S1 re-spelling); its `run_safely` was redundant here — every arm already
    // runs under `contain_panics` at the dispatch point.
    to_value(moss_core::ast::editor_scan::editor_scan(&a.text))
}

/// `parse_frontmatter(file_path)` — reads a vault file, read-only.
///
/// This is the only path-taking arm on the TOKEN-FREE tier, so its confinement
/// is what stands between an unauthenticated same-origin caller and an
/// arbitrary-file read. `read_vault_text` does not confine (its desktop caller
/// has no need to), so the check must happen here.
async fn arm_parse_frontmatter(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
    }
    let a: A = parse_args(args)?;
    let path = confine(ctx, &a.file_path)?;
    let r = crate::editor::content::parse_frontmatter_from_disk(path.to_string_lossy().to_string())
        .await
        .map_err(ArmError::Command)?;
    to_value(r)
}

/// `editor_resolve_asset(target, from_file, state)` — the arm calls the SAME
/// `resolve_asset` core the command body calls, with the project root the
/// command would read from `AppState`.
async fn arm_editor_resolve_asset(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        target: String,
        from_file: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let from_file = confine(ctx, &a.from_file)?;
    let resolved =
        crate::editor::resolve::asset_resolver::resolve_asset(&a.target, &from_file, &root);
    to_value(resolved)
}

/// `editor_resolve_references(targets, from_file, state)` — the arm calls the
/// SAME `resolve_references_batch` core the command body calls.
async fn arm_editor_resolve_references(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        targets: Vec<crate::editor::resolve::reference_resolver::RefTarget>,
        from_file: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let from_file = confine(ctx, &a.from_file)?;
    let resolved = crate::editor::resolve::reference_resolver::resolve_references_batch(
        &a.targets,
        &from_file.to_string_lossy(),
        &root,
    );
    to_value(resolved)
}

// ── Mutation arms (token-gated) ───────────────────────────────────────────────
//
// Each arm calls the SAME pure core its `#[tauri::command]` calls — never a
// reimplementation. `create_files` → `vault::fs::create_files_inner`;
// `save_editor_content` → `editor::commands::persist_editor_content` (the write
// core split out of the command's PanelTask wrapper). The desktop IPC path is
// untouched: those commands still run through Tauri exactly as before.

/// `create_files(files, state)` — the arm calls the SAME `create_files_inner`
/// core the command body calls, with the project root the command reads from
/// `AppState`. Returns the created files' absolute paths, in order (the
/// command's return).
async fn arm_create_files(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        files: Vec<crate::vault::fs::NewFile>,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let created = crate::vault::fs::create_files_inner(&root, &a.files).map_err(ArmError::Command)?;
    to_value(created)
}

/// `create_folder(parentDir, name, state)` — the SAME `create_folder_inner`
/// core the command body calls (`editor/commands.rs`), with the project root
/// the command reads from `AppState`. Returns the created folder's absolute
/// path, as the command does.
///
/// Tiered with `create_files` rather than after it: an editor that can make a
/// page but not a folder to put it in is a half-carried surface, and the tree
/// reports the 404 as an inline error on the create row.
async fn arm_create_folder(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        parent_dir: String,
        name: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let created = crate::vault::fs::create_folder_inner(&root, &a.parent_dir, &a.name)
        .map_err(ArmError::Command)?;
    to_value(created)
}

/// `delete_entry(path)` — the SAME trash-backed core the desktop command calls
/// (`vault::fs::delete_entry_inner`): traversal guard, canonical recheck, root
/// refusal, then OS trash — recoverable, never a permanent unlink. Relative
/// paths join onto the project root, same contract as `confine`.
async fn arm_delete_entry(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let joined;
    let path = if Path::new(&a.path).is_absolute() {
        a.path.as_str()
    } else {
        joined = root.join(&a.path).to_string_lossy().into_owned();
        &joined
    };
    crate::vault::fs::delete_entry_inner(&root, path).map_err(ArmError::Command)?;
    to_value(())
}

/// `save_editor_content(file_path, frontmatter, body, …)` — the arm calls the
/// SAME `persist_editor_content` byte-writing core the command's `run_safely`
/// closure calls. It deliberately does NOT drive the PanelTask lifecycle: there
/// is no action panel / window in a headless HTTP client, and the task machinery
/// is a UI concern layered on top of the identical write (see
/// `editor::commands::save_editor_content_with_task`). The bytes on disk are
/// byte-for-byte what the desktop Save produces — pinned by the dual-path parity
/// test in `router_tests.rs`.
async fn arm_save_editor_content(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
        frontmatter: Value,
        body: String,
    }
    let a: A = parse_args(args)?;
    // The carrier's one arbitrary-write primitive: confine before the write, not
    // after. Token-gated already, but a token is a session credential, not a
    // licence to write outside the vault.
    let path = confine(ctx, &a.file_path)?;
    let r = crate::editor::content::persist_editor_content(
        &path.to_string_lossy(),
        &a.frontmatter,
        &a.body,
    )
    .map_err(ArmError::Command)?;
    to_value(r)
}

// ── Authed-read arms (token-gated) ────────────────────────────────────────────
//
// The editor's boot + open reads. These are the SAME commands the desktop editor
// calls, but gated behind the per-session token (like the mutation tier) rather
// than public: booting the editor exposes the whole vault's tree and every
// source file's role, which is a session-scoped capability, not a public read
// like `parse_frontmatter`. Each arm either calls a `State`-free command body
// directly (`get_file_info`, `compute_heading_state`, `list_directory`) or the
// SAME pure core the `State`-taking command calls (`editor_bootstrap` →
// `bootstrap_for_carrier`, `describe_source_role` → `describe_source_role_inner`).

/// `editor_bootstrap(source_path, …)` — the arm calls the SAME
/// `bootstrap_for_carrier` core, with the project root the command reads from
/// `AppState`. Standalone ⇒ builtin schema, exactly `get_schema`'s no-plugins
/// fallback.
async fn arm_editor_bootstrap(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        source_path: Option<String>,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    // `sourcePath` is optional (a bare boot opens no file); confine it when present.
    let source = a.source_path.as_deref().map(|p| confine(ctx, p)).transpose()?;
    let bootstrap = crate::editor::content::bootstrap_for_carrier(
        &root,
        source.as_ref().map(|p| p.to_string_lossy()).as_deref(),
    );
    to_value(bootstrap)
}

/// `list_directory(path, project_path, show_internal)` — the arm calls the SAME
/// `list_directory_inner` core the command body calls (S1 re-spelling).
///
/// The caller's `projectPath` is **ignored**, deliberately. Over IPC it is the
/// UI restating a root it already knows; over HTTP, honouring it would let the
/// caller supply both the root and the path checked against it — a containment
/// test that validates the input against itself. The authoritative root is the
/// carrier's, and `path` is confined to it.
async fn arm_list_directory(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        path: String,
        show_internal: bool,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let path = confine(ctx, &a.path)?;
    let r = crate::editor::filesystem::list_directory_inner(
        &path.to_string_lossy(),
        &root.to_string_lossy(),
        a.show_internal,
    )
    .map_err(ArmError::Command)?;
    to_value(r)
}

/// `get_file_info(file_path)` — pure-args (no `State`); the arm calls the SAME
/// `get_file_info_inner` core the command body calls (S1 re-spelling).
async fn arm_get_file_info(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
    }
    let a: A = parse_args(args)?;
    let path = confine(ctx, &a.file_path)?;
    let r = crate::editor::content::get_file_info_inner(&path.to_string_lossy())
        .map_err(ArmError::Command)?;
    to_value(r)
}

/// `compute_heading_state(file_path, frontmatter, body)` — pure-args (no
/// `State`); the arm calls the SAME `compute_heading_state_inner` core the
/// command body calls (S1 re-spelling).
async fn arm_compute_heading_state(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
        frontmatter: Value,
        body: String,
    }
    let a: A = parse_args(args)?;
    let path = confine(ctx, &a.file_path)?;
    to_value(crate::editor::content::compute_heading_state_inner(
        &path.to_string_lossy(),
        &a.frontmatter,
        &a.body,
    ))
}

/// `describe_source_role(file_path, state)` — the arm calls the SAME
/// `describe_source_role_inner` core with the project root the command reads from
/// `AppState`.
async fn arm_describe_source_role(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let path = confine(ctx, &a.file_path)?;
    let r = crate::editor::content::describe_source_role_inner(&root, &path.to_string_lossy())
        .map_err(ArmError::Command)?;
    to_value(r)
}

/// `validate_content(file_path)` — the per-file diagnostics the editor shows
/// on every open and every save. Calls the same `editor::validation::diagnose`
/// core the desktop command calls.
///
/// The desktop command resolves the schema from the live plugin manager; the
/// carrier has none loaded, so it composes over an empty plugin set — which is
/// exactly the "no plugins loaded yet, builtin only" branch the desktop command
/// already takes on a cold start. Plugin-contributed fields are therefore not
/// validated over HTTP; they are not silently accepted either, because an
/// unknown field is a warning in both paths.
async fn arm_validate_content(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let path = confine(ctx, &a.file_path)?;
    // A source still in the cloud has nothing to validate; report no
    // diagnostics rather than inventing them from an empty document.
    let Some(content) = crate::editor::content::read_vault_text_off_runtime(
        path.to_string_lossy().into_owned(),
    )
    .await
    .map_err(ArmError::Command)?
    else {
        return to_value(Vec::<crate::editor::validation::EditorDiagnostic>::new());
    };
    let doc = moss_core::frontmatter::parse(&content);
    let schema = crate::editor::validation::compose_schema(&[]);
    to_value(crate::editor::validation::diagnose(
        Some(&root),
        &path,
        &doc.frontmatter,
        &schema,
    ))
}

/// `list_tree(path, show_internal)` — the file-tree walk. The command's `path`
/// arg is the project root (that is how `FileTree` calls it), used for both the
/// walk root and the project root, matching the command body. The uncached
/// `list_tree_inner` recomputes publish dates each call — the carrier has no
/// long-lived `AppState` publish-date memo, and a per-call recompute is correct,
/// just not memoized.
/// The caller's `path` is ignored for the same reason `list_directory` ignores
/// `projectPath`: it serves as BOTH the walk root and the project root, so
/// honouring it would let the caller walk any directory on the machine and
/// declare it the project. The carrier's own root is authoritative.
async fn arm_list_tree(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        show_internal: bool,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let root_s = root.to_string_lossy();
    let r = crate::editor::filesystem::list_tree_inner(&root_s, &root_s, a.show_internal)
        .map_err(ArmError::Command)?;
    to_value(r)
}

/// `resolve_url_for_file(file_path, state)` — the accurate post-build resolve
/// PreviewFollower uses to point the preview at the edited page. Coordinates
/// editor→preview scroll; the preview IS build output, so consulting it here is
/// not the editor-renders-source concern. The arm mirrors the
/// command body: strip the project root the command reads from `AppState`, then
/// call the SAME pure `resolve_url_for_file_inner` core.
///
/// Known gap: unlike the Tauri command (which asks the live `ServerState` for
/// this folder's actual served dir), this arm passes `initial_serve_dir()` —
/// the same cold-start-only heuristic `resolve_url_for_file_inner` used to
/// compute internally before it took a `served_dir` parameter. `Session`
/// (see `session.rs`) resolves the vault from the live `site_dir` at bind
/// time but does not retain the pointer itself, and the `carrier!` macro
/// hands every arm in a tier the same `(ctx, args)` pair, so reaching the
/// live dir here would mean widening that shared signature (or adding a
/// field to `Session`) for one arm out of eight. Out of scope for the fix
/// that added `served_dir` — a page staged by a rebuild after this vault's
/// first seal can still miss here even though the Tauri command now finds
/// it.
async fn arm_resolve_url_for_file(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        file_path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    // `confine` subsumes the containment half of the command body's strip_prefix
    // and adds the symlink recheck; the strip itself still yields the relative
    // path the pure core wants.
    let abs = confine(ctx, &a.file_path)?;
    let rel = abs
        .strip_prefix(root.canonicalize().as_deref().unwrap_or(&root))
        .or_else(|_| abs.strip_prefix(&root))
        .map_err(|_| {
            ArmError::Command(format!(
                "File '{}' is not inside project '{}'",
                a.file_path,
                root.display()
            ))
        })?
        .to_string_lossy()
        .to_string();
    let served_dir =
        crate::moss_paths::MossPaths::from_moss_dir(root.join(".moss")).initial_serve_dir();
    let r = crate::editor::resolve::links::resolve_url_for_file_inner(&rel, &root, &served_dir)
        .map_err(ArmError::Command)?;
    to_value(r)
}

// ── Versions arms (publish history) ───────────────────────────────────────────
//
// The five commands of the Versions surface, each calling the SAME
// `deploy::history::panel` body its `#[tauri::command]` calls — never a
// reimplementation, exactly like the editor arms above.
//
// The one thing that differs between the carriers is where "the tree as it
// stands right now" comes from. The app hands its body the sealed manifest
// `AppState` holds; these hand it `one_shot::build_sealed_now`, which builds,
// because a headless process has no live manifest — the same answer
// `moss history --save` has made since slice 2. The provider is a future the
// body awaits only when it needs one, so `list_versions` builds on the
// site-version drill-down and never on the timeline an open panel asks for.
//
// `projectPath` is IGNORED on all five, for the reason `list_directory`
// ignores it: the carrier's own vault is authoritative, and honouring a
// caller-supplied root would let the request name the folder it acts on.
// `id` is caller-supplied here in a way it never is over IPC, and the body
// checks it against the records the store lists before the store joins it into
// a filename (`panel::known_id`).

/// `list_versions(project_path, scope, path, state)` — the Versions list for
/// either scope, and the site-version drill-down. Only the drill-down awaits
/// the manifest provider, so an opened panel costs no build.
async fn arm_list_versions(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        scope: String,
        path: Option<String>,
    }
    let a: A = parse_args(args)?;
    let vault = ctx.vault();
    let r = crate::deploy::history::panel::list_versions(
        vault.path(),
        &a.scope,
        a.path,
        crate::deploy::one_shot::build_sealed_now(vault),
    )
    .await
    .map_err(ArmError::Command)?;
    to_value(r)
}

/// `read_version(project_path, id, path)` — pure-args (no `State`); one
/// version's bytes at one path, as text or as "not kept".
async fn arm_read_version(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        id: String,
        path: String,
    }
    let a: A = parse_args(args)?;
    // `path` needs no `confine`: it is a key into the record's own entry map,
    // never joined onto anything, and a key the record does not carry is
    // refused by name before the object store is touched.
    let r = crate::deploy::history::panel::read_version(ctx.vault().path(), &a.id, &a.path)
        .map_err(ArmError::Command)?;
    to_value(r)
}

/// `reveal_history_store(project_path)` — "Show in Finder" on this vault's
/// history store. The carrier is loopback-only, so the file manager this opens
/// is on the same machine as the caller, exactly as on the desktop path; the
/// path itself is moss's own fixed subpath of the vault, with nothing
/// caller-supplied in it.
async fn arm_reveal_history_store(ctx: &Session, _args: Value) -> ArmResult {
    crate::deploy::history::panel::reveal_history_store(ctx.vault().path())
        .map_err(ArmError::Command)?;
    to_value(())
}

/// `restore_version(project_path, id, path, mode, state)` — restore one page
/// or the whole site, after saving the present as a version first. The site
/// form moves pages added since the version to the OS Trash (recoverable,
/// never an unlink) through the same delete core `delete_entry` uses.
async fn arm_restore_version(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        id: String,
        path: Option<String>,
        mode: String,
    }
    let a: A = parse_args(args)?;
    let vault = ctx.vault();
    let r = crate::deploy::history::panel::restore_version(
        vault.path(),
        &a.id,
        a.path,
        &a.mode,
        crate::deploy::one_shot::build_sealed_now(vault),
    )
    .await
    .map_err(ArmError::Command)?;
    to_value(r)
}

/// `save_version(project_path, label, app, state)` — save a version now.
///
/// The desktop command waits for an in-flight watch rebuild before reading
/// `AppState`'s manifest; this arm's provider IS a build, which takes the
/// per-folder stage-write lock and so is already ordered against the watcher's.
async fn arm_save_version(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        label: Option<String>,
    }
    let a: A = parse_args(args)?;
    let vault = ctx.vault();
    let r = crate::deploy::history::panel::save_version(
        vault.path(),
        a.label,
        crate::deploy::one_shot::build_sealed_now(vault),
    )
    .await
    .map_err(ArmError::Command)?;
    to_value(r)
}

// ── The one manual surface: allowlist + generated dispatch ────────────────────

/// Generates BOTH a carrier allowlist (`$list`) and its dispatch fn (`$dispatch`)
/// from ONE list of `command_name => arm` pairs. The names double as the command
/// names, so there is exactly one place to edit per tier and a name can never
/// drift from its arm. Every `command_name` MUST be registered in `command_list!`
/// (`registry.rs`) — enforced across BOTH lists by
/// `allowlist_is_a_subset_of_the_command_registry`.
///
/// One macro, two invocations (read-only + mutation), so both tiers share the
/// same "one list, validated subset" discipline.
/// The carrier's ONE panic boundary: every dispatched arm runs under this
/// `catch_unwind`, so containment is a property of the transport, not of each
/// core — an arm that calls a core directly (no `run_safely`) is still
/// contained, and the S1 re-spelling of the remaining `commands::*` arms
/// cannot lose it. A caught panic becomes a 500 (`ArmError::Command`), the
/// same envelope as a command's own `Err(String)`, instead of unwinding
/// through hyper. Dev-profile containment only: release builds set
/// `panic = "abort"` (workspace `Cargo.toml`), where no unwind exists.
async fn contain_panics(
    label: &'static str,
    arm: impl std::future::Future<Output = ArmResult>,
) -> ArmResult {
    use futures::FutureExt;
    match std::panic::AssertUnwindSafe(arm).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            let msg = crate::editor::content::panic_message_from_payload(&*payload);
            log::error!("Carrier arm `{label}` panicked: {msg}");
            Err(ArmError::Command(format!("Internal error in `{label}`: {msg}")))
        }
    }
}

macro_rules! carrier {
    ($(#[$m:meta])* $list:ident, $dispatch:ident { $($cmd:ident => $arm:path),+ $(,)? }) => {
        $(#[$m])*
        pub const $list: &[&str] = &[$(stringify!($cmd)),+];

        /// `None` ⇒ the command is not in this tier's allowlist ⇒ the router
        /// returns 404 (absent, not gated).
        async fn $dispatch(ctx: &Session, cmd: &str, args: Value) -> Option<ArmResult> {
            match cmd {
                $(stringify!($cmd) => Some(contain_panics(stringify!($cmd), $arm(ctx, args)).await),)+
                _ => None,
            }
        }
    };
}

carrier! {
    /// The read-only subset of `command_list!` exposed over `POST
    /// /__moss/invoke/<cmd>`. Token-FREE (public-read). A strict subset of the
    /// registry, validated by test.
    READ_ONLY_HTTP_COMMANDS, dispatch {
        scan_shortcodes => arm_scan_shortcodes,
        parse_frontmatter => arm_parse_frontmatter,
        editor_resolve_asset => arm_editor_resolve_asset,
        editor_resolve_references => arm_editor_resolve_references,
    }
}

carrier! {
    /// The MUTATION subset of `command_list!` exposed over `POST
    /// /__moss/mutate/<cmd>`. Token-GATED (`X-Moss-Token`). A strict subset of
    /// the registry, validated by the SAME subset test as the read-only list.
    /// Kept minimal: exactly the commands the acceptance flow needs — create a
    /// file, persist edited page bytes to disk, and the two Versions actions
    /// that write into the vault (a restore overwrites and trashes; a save
    /// writes a record and its blobs).
    MUTATION_HTTP_COMMANDS, dispatch_mutation {
        create_files => arm_create_files,
        create_folder => arm_create_folder,
        delete_entry => arm_delete_entry,
        save_editor_content => arm_save_editor_content,
        restore_version => arm_restore_version,
        save_version => arm_save_version,
    }
}

carrier! {
    /// The AUTHED-READ subset of `command_list!` exposed over `POST
    /// /__moss/read/<cmd>`. Token-GATED (`X-Moss-Token`), the SAME gate as the
    /// mutation tier — booting the editor exposes the whole vault, so it is a
    /// session-scoped capability, not a public read like `parse_frontmatter`.
    /// A strict subset of the registry, validated by the SAME subset test as the
    /// other two lists. These are the editor's boot + open reads, plus the
    /// Versions surface's three non-writing commands. `list_versions` can build
    /// the site to answer a site-version drill-down (that is how a headless
    /// process gets a sealed manifest at all) — it writes moss's own output
    /// tree, never the author's files, which is what keeps it a read.
    AUTHED_READ_HTTP_COMMANDS, dispatch_authed_read {
        editor_bootstrap => arm_editor_bootstrap,
        list_directory => arm_list_directory,
        list_tree => arm_list_tree,
        get_file_info => arm_get_file_info,
        compute_heading_state => arm_compute_heading_state,
        describe_source_role => arm_describe_source_role,
        resolve_url_for_file => arm_resolve_url_for_file,
        validate_content => arm_validate_content,
        list_versions => arm_list_versions,
        read_version => arm_read_version,
        reveal_history_store => arm_reveal_history_store,
    }
}

/// Turn a dispatch outcome (`None` ⇒ not on this tier's allowlist) into the HTTP
/// response. `None` ⇒ 404 (absent, not gated). Shared by both handlers so the
/// two tiers map arm results to status codes identically.
fn dispatch_outcome_to_response(cmd: &str, outcome: Option<ArmResult>) -> Response {
    match outcome {
        None => (
            StatusCode::NOT_FOUND,
            format!("unknown or unexposed command: {cmd}"),
        )
            .into_response(),
        Some(Ok(value)) => (StatusCode::OK, Json(value)).into_response(),
        Some(Err(ArmError::BadArgs(msg))) => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
        }
        Some(Err(ArmError::Command(msg))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// Route handler for `POST /__moss/invoke/*cmd` — the READ-ONLY carrier.
///
/// `site_dir` is the server's live directory pointer (`<vault>/.moss/build.nosync/…`);
/// the vault root walked up from it is the project root the `State` commands
/// would read. It is threaded in from `build_router` exactly as `asset_registry`
/// is. Token-free, so no gate has bound for it: it binds, then dispatches
/// against the session it took. A serve dir outside any vault answers every
/// allowlisted command with [`NO_PROJECT_OPEN`], exactly as
/// `AppState::require_project_path` does with no folder open — its one pure
/// arm (`scan_shortcodes`) included, so that no arm has to carry an unbound case.
///
/// [`NO_PROJECT_OPEN`]: crate::vault::paths::NO_PROJECT_OPEN
pub async fn handle_invoke(
    ctx: InvokeCtx,
    site_dir: Arc<RwLock<PathBuf>>,
    cmd: String,
    args: Value,
) -> Response {
    let outcome = match ctx.bind(&site_dir) {
        Some(session) => dispatch(&session, &cmd, args).await,
        None if READ_ONLY_HTTP_COMMANDS.contains(&cmd.as_str()) => Some(Err(ArmError::Command(
            crate::vault::paths::NO_PROJECT_OPEN.to_string(),
        ))),
        None => None,
    };
    dispatch_outcome_to_response(&cmd, outcome)
}

/// Route handler for `POST /__moss/mutate/*cmd` — the MUTATION carrier.
///
/// Dispatches against the mutation allowlist. The token/method/content-type
/// gate is NOT here — it is the `route_layer` this route carries (see
/// `router.rs` + `super::carrier_token`), which runs before this handler,
/// after the outer trust boundary. It binds the session from the live
/// `site_dir`, compares the token, and hands the admitted [`Session`] down as
/// a request extension. So by the time execution reaches this function the
/// request is already same-origin, loopback, POST, `application/json`,
/// token-authenticated, and pinned to the vault the token was minted for.
pub async fn handle_mutate(
    axum::Extension(session): axum::Extension<Arc<Session>>,
    cmd: String,
    args: Value,
) -> Response {
    dispatch_outcome_to_response(&cmd, dispatch_mutation(&session, &cmd, args).await)
}

/// Route handler for `POST /__moss/read/*cmd` — the AUTHED-READ carrier.
///
/// Identical plumbing to [`handle_mutate`] but dispatching against the
/// authed-read allowlist. It carries the SAME token `route_layer` the mutation
/// route does (see `router.rs`), so the same admitted session arrives the same
/// way. The tier is in the URL path, so the gate is structural.
pub async fn handle_authed_read(
    axum::Extension(session): axum::Extension<Arc<Session>>,
    cmd: String,
    args: Value,
) -> Response {
    dispatch_outcome_to_response(&cmd, dispatch_authed_read(&session, &cmd, args).await)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The transport's panic boundary: a panicking arm must come back as the
    /// 500-shaped `ArmError::Command`, not unwind through dispatch (and
    /// hyper). Exercised through `contain_panics` because the `carrier!`
    /// macro routes EVERY arm of every tier through it — there is no
    /// uncontained dispatch path for this to miss.
    #[tokio::test]
    async fn a_panicking_arm_returns_an_error_instead_of_unwinding() {
        let r = contain_panics("boom_cmd", async {
            panic!("the core exploded");
            #[allow(unreachable_code)]
            Ok(Value::Null)
        })
        .await;
        match r {
            Err(ArmError::Command(msg)) => {
                assert!(msg.contains("Internal error in `boom_cmd`"), "got: {msg}");
                assert!(msg.contains("the core exploded"), "got: {msg}");
            }
            other => panic!("expected a contained Command error, got {other:?}"),
        }
    }

    /// A vault-relative path confines to the file under the root — the shape
    /// the shipping editor sends (`fromFile: "note.md"`); rejecting it forked
    /// the carrier from the desktop command path, which resolves relative
    /// paths against the open project. Escapes must still fail after the join.
    #[test]
    fn confine_accepts_project_relative_paths_and_still_rejects_escapes() {
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

    /// `delete_entry` is ON the mutation carrier (the file tree's delete failed
    /// with a 404 in a browser, 2026-09-01), and its arm holds the same
    /// line the desktop path holds: escapes and the project root are refused.
    /// The success path is not exercised here — it would move a real file into
    /// the OS trash, and the shared core's guards are what this test pins.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_entry_arm_is_mounted_and_confined() {
        let (dir, ctx) = scratch();

        let refused = |v: Value| async {
            match dispatch_mutation(&ctx, "delete_entry", v).await {
                Some(Err(_)) => {}
                other => panic!("expected a refused dispatch, got {other:?}"),
            }
        };
        refused(json!({ "path": "../outside.md" })).await;
        refused(json!({ "path": "/etc/passwd" })).await;
        refused(json!({ "path": dir.path().to_string_lossy() })).await; // the root itself
    }

    // The allowlist-is-a-subset-of-the-registry gate lives app-side
    // (in the desktop app's preview server tests): it reads `registry.rs`, which
    // stays in the app crate — the one manual surface is still validated
    // against the ONE `command_list!`, just from the crate that owns it.

    /// The three tiers are pairwise disjoint. This is what makes the URL path a
    /// genuine trust boundary — a mutation cannot be reached on the token-free
    /// `/invoke` route, a token-gated editor read cannot leak onto it either, and
    /// a public read cannot accidentally demand a token on `/mutate` or `/read`.
    ///
    /// Intersecting the three lists, rather than spot-checking eight hand-picked
    /// names as this test used to: a sample only fails for the commands someone
    /// remembered, and a command added to two tiers is exactly the mistake
    /// nobody would think to sample for. `carrier!` derives each list and its
    /// dispatcher from ONE source list, so the lists are the dispatchers; the
    /// assertion below pins that derivation so this stays true if the macro
    /// changes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_three_tiers_are_disjoint() {
        let tiers: [(&str, &[&str]); 3] = [
            ("read-only (token-free)", READ_ONLY_HTTP_COMMANDS),
            ("mutation", MUTATION_HTTP_COMMANDS),
            ("authed-read", AUTHED_READ_HTTP_COMMANDS),
        ];
        for (i, (name_a, a)) in tiers.iter().enumerate() {
            for (name_b, b) in tiers.iter().skip(i + 1) {
                let shared: Vec<&&str> = a.iter().filter(|c| b.contains(c)).collect();
                assert!(
                    shared.is_empty(),
                    "{name_a} and {name_b} share {shared:?}; the URL path must fix the tier"
                );
            }
        }

        // The lists ARE the dispatchers — one `carrier!` list generates both.
        // One command per tier, checked against the two dispatchers it must not
        // answer, is what would go red if that stopped being so.
        let (_dir, ctx) = scratch();
        assert!(dispatch(&ctx, "save_editor_content", json!({})).await.is_none());
        assert!(dispatch_mutation(&ctx, "scan_shortcodes", json!({})).await.is_none());
        assert!(dispatch_authed_read(&ctx, "save_editor_content", json!({})).await.is_none());
    }

    /// A command not in the allowlist dispatches to `None`, which the router
    /// turns into a 404 (absent, not gated).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unlisted_command_dispatches_to_none() {
        let (_dir, ctx) = scratch();
        assert!(
            dispatch(&ctx, "reveal_entry", json!({})).await.is_none(),
            "a desktop-only/unlisted command must be absent from this carrier"
        );
        assert!(
            dispatch(&ctx, "no_such_command_xyz", json!({})).await.is_none(),
        );
    }

    /// A listed pure-args command dispatches to `Some(Ok(_))` and returns the
    /// command's real result.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_shortcodes_dispatches_and_returns_a_result() {
        let (_dir, ctx) = scratch();
        let out = dispatch(&ctx, "scan_shortcodes", json!({ "text": "# hi\n\nplain" }))
            .await
            .expect("listed command must dispatch")
            .expect("scan_shortcodes is infallible on valid text");
        // EditorScanResult is an object — proves we got a real serialized result.
        assert!(out.is_object(), "expected an EditorScanResult object, got: {out}");
    }

    /// Bad args surface as `BadArgs`, mapped to 400 by the handler.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_required_arg_is_bad_args() {
        let (_dir, ctx) = scratch();
        let r = dispatch(&ctx, "scan_shortcodes", json!({}))
            .await
            .expect("listed")
            .expect_err("missing 'text' must be an error");
        assert!(matches!(r, ArmError::BadArgs(_)), "missing arg must be BadArgs (→400)");
    }
}
