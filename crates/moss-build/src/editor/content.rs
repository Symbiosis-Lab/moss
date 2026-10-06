//! Pure editor-content cores — the tauri-free halves of `commands.rs`.
//!
//! Every function here is a plain data-in/data-out (or filesystem-only) core
//! that a `#[tauri::command]` wrapper in `commands.rs` calls, and that the HTTP
//! command carrier (`preview::server::invoke`) reaches without a Tauri shell.
//! Nothing in this file may name a `tauri::` type: this is the split line the
//! editor-backend relocation (preview-server relocation plan, slice E2) crosses,
//! and rust-analyzer + the workspace suite check it here first, in place.

use super::filesystem::EditorFileInfo;

// ── Panic boundary ─────────────────────────────────────────────────────────

/// Run a sync command body with `catch_unwind`. On panic, log the payload and
/// convert to an `Err(String)` returned to the caller.
///
/// This is the containment for the *Tauri wrappers* (and the sync closures
/// they hand it). The HTTP carrier does NOT rely on it per-arm: its
/// containment is one `catch_unwind` at the dispatch point
/// (`preview::server::invoke`), so it holds whether or not an arm's core
/// happens to route through here. Both catches are dev-profile containment
/// only — release builds set `panic = "abort"`, where no unwind exists to
/// catch.
pub fn run_safely<T>(
    label: &'static str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let msg = panic_message_from_payload(&*payload);
            log::error!("Command `{label}` panicked: {msg}");
            Err(format!("Internal error in `{label}`: {msg}"))
        }
    }
}

/// Best-effort human text from a panic payload. Shared by [`run_safely`] and
/// the HTTP carrier's dispatch-point catch.
pub fn panic_message_from_payload(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "non-string panic payload".to_string()
    }
}

// ── Wire types ─────────────────────────────────────────────────────────────

/// Result of a successful editor save — exposes the new mtime so the
/// frontend's FrontmatterStore can register the self-write and drop the
/// subsequent file-watcher echo.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SaveResult {
    pub mtime_ms: u64,
}

/// Frontmatter parsing result exposed to the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ParsedFrontmatter {
    /// Parsed frontmatter as a JSON value (object).
    pub frontmatter: serde_json::Value,
    /// The markdown body (everything after the closing `---`).
    pub body: String,
    /// Whether the document had frontmatter delimiters.
    pub has_frontmatter: bool,
    /// serde_yaml error message when a delimited `---...---` block failed to
    /// parse. `None` when it parsed cleanly or there was no block. The frontend
    /// can render an "invalid YAML" banner off this so a malformed block flags an
    /// error instead of silently showing a blank frontmatter form. `body` still
    /// holds the whole document so the author can repair the raw block in CM6.
    #[serde(default)]
    pub frontmatter_error: Option<String>,
    /// The file is real and its bytes are not here yet — a cloud placeholder
    /// the provider has not handed back. Every other field is
    /// empty, and the editor must mount its "still arriving" pane rather than
    /// this document: an empty CM6 buffer over a file with content is the same
    /// lie as the errno toast this replaced.
    ///
    /// Set only by [`classify_source_read`], which is the one place in the
    /// editor that reads the cloud's answer. Never true together with content.
    #[serde(default)]
    pub still_arriving: bool,
}

impl ParsedFrontmatter {
    /// The document for a source whose bytes are still in the cloud.
    pub fn still_arriving() -> Self {
        Self {
            frontmatter: serde_json::Value::Object(serde_json::Map::new()),
            body: String::new(),
            has_frontmatter: false,
            frontmatter_error: None,
            still_arriving: true,
        }
    }
}

/// Everything the editor webview needs for first paint, in one IPC round trip.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EditorBootstrap {
    /// Composed content schema (builtin + plugin fields) — same source as `get_schema`.
    pub schema: moss_core::schema::ContentSchema,
    /// Project root, or None when no project is open (the editor still boots;
    /// the file tree is skipped).
    pub project_path: Option<String>,
    /// Best-effort parse of the editor URL's `source` param. None when there is
    /// no source, it isn't markdown, or the parse failed — the frontend falls
    /// back to `parse_frontmatter`, which re-surfaces the real error through
    /// the existing logging path.
    pub initial_file: Option<ParsedFrontmatter>,
    /// THE project-root folder name; `None` with no project open, or for `/`.
    /// EMITTED, not re-derived (same rule as `TreeNode.is_home`): the site's home
    /// note is `<root_name>.md`, and only a name matching the REAL root basename is
    /// elected as `/` by `moss_core::home::is_home_file`.
    pub root_name: Option<String>,
    /// The editor's path-domain root when there is NO project: the loose file's
    /// parent directory (document mode).
    ///
    /// Never `Some` at the same time as `project_path` — they are the two arms
    /// of one choice, kept as separate fields so the frontend can tell "a site
    /// I can preview" from "a folder I merely need for path arithmetic". The
    /// editor's identity registry is project-root-relative and *throws* on an
    /// absolute path, so a document-mode editor still needs a root even though
    /// nothing about that folder is a site — and emitting `project_path` for it
    /// would be a lie the preview and deploy paths would act on.
    pub editor_root: Option<String>,
}

/// What a source file becomes on the built site. "Which slot, if any" rather
/// than a boolean, so a future slot file needs no new command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SourceRole {
    /// Layout slot this file fills INSTEAD of becoming a page (`"footer-left"`).
    pub slot: Option<String>,
    /// Language tree the file lives in (`"zh-hans"`); `None` for the default
    /// tree. Reported for EVERY file, not just slot files: the shell compares
    /// the tree of the slot file being edited against the tree of the page the
    /// preview is showing, and needs the same answer for both sides of that
    /// comparison. One command, one definition of "which language tree" —
    /// asking a second command for the page side would be a place to drift.
    pub language: Option<String>,
    /// Whether the build turns this file's extension into a page at all.
    ///
    /// Reported so the preview follower can tell "not built YET" (keep
    /// retrying on the next build) from "never a page" (stop, and say so).
    /// It answers the extension question only — `slot` above still overrides
    /// it, and `.moss/` inputs are a path fact the frontend already owns.
    ///
    /// This rides on `describe_source_role` rather than becoming a set in
    /// TypeScript for the same reason `slot` does: the authority is
    /// `build::scan::classify::is_page_source`, and a second copy of it is the
    /// Rust↔TS parity trap.
    pub can_be_page: bool,
}

// ── Reading a source file (the pending-read seam) ─────────────────────────────

/// Reads a vault text file for a user-initiated action, waiting briefly for a
/// cloud-evicted source. Someone is watching, so the wait is short and bounded.
///
/// **Blocks.** Call it from `spawn_blocking` (see
/// `commands::read_vault_text_off_runtime`) or from a genuinely synchronous
/// context — never directly from an `async` command body.
pub fn read_vault_text(file_path: &str) -> Result<Option<String>, String> {
    let path = std::path::Path::new(file_path);
    classify_source_read(
        path,
        crate::build::cloud_readiness::read_to_string_with_materialize_wait(
            path,
            crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
        ),
    )
}

/// The editor's ONE translation of a failed source read.
///
/// `Ok(None)` — "still arriving" — is reserved for
/// [`crate::build::icloud::is_offline_not_absent`], the same classifier
/// `cloud_readiness::retry_after_materialize` and the build's `outcome::io_stop`
/// consult. There is no second notion of "offline vs absent" in moss, and this
/// is not one: it is the build half's seam, read from the editor side.
///
/// The polarity mirrors `cloud_readiness::probe_input_to_string`, where
/// `Ok(None)` is likewise `Found::InCloud` — one spelling of the answer on both
/// halves.
///
/// Why it matters that this is the only place: reading a dataless file under
/// `set_dataless_fail_fast` fails with EDEADLK, and until this existed that
/// `io::Error` was formatted into the same string every other read failure
/// produced. It reached the user as `Resource deadlock avoided (os error 11)`
/// on a toast, over a home page that existed and was merely downloading — and
/// the failed open left the PREVIOUS pane mounted, so the editor also said
/// "there's nothing here to edit" about it.
///
/// Split from [`read_vault_text`] so the EDEADLK arm is testable without an
/// evicted file, which no test can create.
fn classify_source_read(
    path: &std::path::Path,
    read: std::io::Result<String>,
) -> Result<Option<String>, String> {
    match read {
        Ok(text) => Ok(Some(text)),
        Err(e) if crate::build::icloud::is_offline_not_absent(path, &e) => {
            log::info!(
                "[editor] {} is still in the cloud — showing the waiting pane, not an error",
                path.display()
            );
            Ok(None)
        }
        Err(e) => Err(format!("Failed to read file '{}': {}", path.display(), e)),
    }
}

/// [`read_vault_text`], on a thread that is allowed to block.
///
/// Async callers run on tokio *worker* threads, and the bounded wait inside is
/// a `std::thread::sleep` poll loop — up to 15 s of a worker parked doing
/// nothing. Enough concurrent editor opens on a cloud vault and every async
/// command in the app starves behind them. `spawn_blocking` is the pool that
/// exists for this; `source_asset.rs` does the same. `tokio::task`, not
/// `tauri::async_runtime`: every caller (Tauri command, axum handler) already
/// executes inside a tokio runtime, so a context exists and this file stays
/// tauri-free.
pub async fn read_vault_text_off_runtime(
    file_path: String,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || read_vault_text(&file_path))
        .await
        .map_err(|e| format!("read task failed: {}", e))?
}

/// The whole of `parse_frontmatter`: off-runtime read, then the pure parse
/// under [`run_safely`]; a cloud placeholder yields the still-arriving
/// document, not an error.
pub async fn parse_frontmatter_from_disk(
    file_path: String,
) -> Result<ParsedFrontmatter, String> {
    match read_vault_text_off_runtime(file_path).await? {
        Some(content) => run_safely("parse_frontmatter", move || parse_frontmatter_of(content)),
        // Not an error, and deliberately not an empty document: the editor
        // mounts its "still arriving" pane and re-resolves when the bytes land.
        None => Ok(ParsedFrontmatter::still_arriving()),
    }
}

/// Read and parse in one call, for genuinely synchronous callers.
///
/// Async callers must not use this: it blocks. They read via
/// [`read_vault_text_off_runtime`] and parse with
/// [`parse_frontmatter_of`], which is pure.
#[cfg(test)]
fn parse_frontmatter_at(file_path: &str) -> Result<ParsedFrontmatter, String> {
    match read_vault_text(file_path)? {
        Some(content) => parse_frontmatter_of(content),
        None => Ok(ParsedFrontmatter::still_arriving()),
    }
}

/// The pure half: everything `parse_frontmatter` does once the bytes are in
/// hand. Split out so the blocking read can happen off the async runtime.
pub fn parse_frontmatter_of(content: String) -> Result<ParsedFrontmatter, String> {
    let doc = moss_core::frontmatter::parse(&content);

    // Convert the HashMap<String, serde_yaml::Value> to serde_json::Value
    // for consistent JSON transport to the frontend.
    let fm_json = super::frontmatter::yaml_map_to_json(&doc.frontmatter)?;

    Ok(ParsedFrontmatter {
        frontmatter: fm_json,
        body: doc.body,
        has_frontmatter: doc.frontmatter_range.is_some(),
        frontmatter_error: doc.frontmatter_error,
        still_arriving: false,
    })
}

// ── editor_bootstrap cores ─────────────────────────────────────────────────

/// The editor's path-domain root when there is no project (document
/// mode): the loose file's parent directory.
///
/// A project always wins — the two are the arms of one choice, never both.
/// Pure so the choice is testable without a Tauri `State`.
pub fn bootstrap_editor_root(
    project_path: Option<&str>,
    document_file: Option<&std::path::Path>,
) -> Option<String> {
    if project_path.is_some() {
        return None;
    }
    document_file?
        .parent()
        .map(|p| p.to_string_lossy().to_string())
}

/// THE root name for the editor bootstrap: resolved once, by the owner.
fn bootstrap_root_name(project_path: Option<&str>) -> Option<String> {
    crate::vault_root::VaultRoot::resolve(project_path?)
        .name_opt()
        .map(str::to_owned)
}

/// Resolve the editor URL's `source` param to an absolute path. The param is
/// typically absolute (baked by `build_editor_url` from `resolve_page_source`),
/// but the file-tree event domain is project-root-relative — accept both:
/// absolute passes through, relative joins onto the project path (mirror of
/// the app's own `toAbsolute`).
fn resolve_bootstrap_source(project_path: Option<&str>, source: &str) -> Option<String> {
    if std::path::Path::new(source).is_absolute() {
        return Some(source.to_string());
    }
    project_path.map(|root| {
        std::path::Path::new(root)
            .join(source)
            .to_string_lossy()
            .to_string()
    })
}

/// Which file `editor_bootstrap` should pre-parse, if any: only `.md`/
/// `.markdown` sources (case-insensitive), resolved to an absolute path.
///
/// Pure — no I/O — so the async command can do the read itself, off the
/// runtime. `None` here (and any later failure) just means the frontend falls
/// back to its existing `parse_frontmatter` call.
pub fn bootstrap_source_path(
    project_path: Option<&str>,
    source: Option<&str>,
) -> Option<String> {
    let source = source?;
    let is_markdown = std::path::Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false);
    if !is_markdown {
        return None;
    }
    resolve_bootstrap_source(project_path, source)
}

/// Read-and-parse counterpart of [`bootstrap_source_path`], for synchronous
/// callers. **Blocks** — see [`read_vault_text`] — which is why production goes
/// through `bootstrap_source_path` + `read_vault_text_off_runtime` instead.
#[cfg(test)]
fn bootstrap_initial_file(
    project_path: Option<&str>,
    source: Option<&str>,
) -> Option<ParsedFrontmatter> {
    parse_frontmatter_at(&bootstrap_source_path(project_path, source)?).ok()
}

/// The pure assembly half of `editor_bootstrap`: given the two roots, the
/// composed schema, and the initial file's raw content (already read off the
/// runtime), build the `EditorBootstrap`. No I/O, no `State` — so the HTTP
/// carrier can call it with the identical inputs and produce the identical
/// bootstrap. This is the "one core, two carriers" seam for the boot payload.
pub fn assemble_bootstrap(
    project_path: Option<String>,
    editor_root: Option<String>,
    schema: moss_core::schema::ContentSchema,
    initial_content: Option<String>,
) -> EditorBootstrap {
    let root = project_path.as_deref().or(editor_root.as_deref());
    let initial_file = initial_content.and_then(|c| parse_frontmatter_of(c).ok());
    let root_name = bootstrap_root_name(root);
    EditorBootstrap {
        schema,
        project_path,
        initial_file,
        root_name,
        editor_root,
    }
}

/// The HTTP carrier's boot core: given a project root and the editor URL's
/// optional `source` param, produce the SAME `EditorBootstrap` the Tauri command
/// produces in standalone (no-plugins) mode.
///
/// A carrier always has a project (seeded from the live serve dir), so
/// `editor_root` — the no-project document-mode root — is always `None` here.
/// The schema is `builtin_schema()`, which is exactly `get_schema`'s fallback
/// when no plugin managers are loaded; plugins are a desktop-shell concern the
/// headless carrier does not carry. The initial file is read synchronously: the
/// carrier's axum handler is already off the UI thread, so the bounded
/// cloud-wait inside `read_vault_text` is fine here (unlike the async command,
/// which must push it to `spawn_blocking`).
pub fn bootstrap_for_carrier(
    project_root: &std::path::Path,
    source_path: Option<&str>,
) -> EditorBootstrap {
    let project_path = Some(project_root.to_string_lossy().to_string());
    let schema = moss_core::schema::builtin_schema();
    let initial_content = bootstrap_source_path(project_path.as_deref(), source_path)
        .and_then(|abs| read_vault_text(&abs).ok().flatten());
    assemble_bootstrap(project_path, None, schema, initial_content)
}

// ── Heading state, file info, source role ──────────────────────────────────

/// Core of `compute_heading_state`: the JSON `Value` → `HeadingInputs` bridge
/// over `moss_core::heading::compute` — the single source of truth shared with
/// the build pipeline's `<h1 class="moss-article-title">` injection gate
/// (`build::markdown::pipeline`).
pub fn compute_heading_state_inner(
    file_path: &str,
    frontmatter: &serde_json::Value,
    body: &str,
) -> moss_core::heading::HeadingState {
    let title_str = match frontmatter.get("title") {
        Some(serde_json::Value::String(s)) => Some(s.as_str()),
        _ => None,
    };

    // Apply CriticMarkup acceptance before hero-from-body detection so the
    // editor sees the same normalized body the build pipeline does
    // (pipeline.rs:103). Otherwise an author with a leading `{>>comment<<}`
    // before `:::hero` would see the editor and pipeline disagree about
    // hero ownership of the heading slot.
    let accepted_body = crate::build::markdown::html_post::accept_criticmarkup(body);

    moss_core::heading::compute(moss_core::heading::HeadingInputs {
        file_path,
        frontmatter_title: title_str,
        body_markdown: &accepted_body,
        // The editor command receives a workspace-relative file_path, but
        // doesn't have the workspace folder's basename here. Self-named
        // home detection at the project root will fall through to the
        // path-based check (i.e. won't catch `<root>/<root>.md`). The
        // editor's heading state is already a "best effort" preview vs
        // the build pipeline's authoritative answer; this is one of the
        // pre-existing gaps documented at HeadingInputs::root_folder_name.
        root_folder_name: None,
        is_home_override: false,
        // PR7b: the editor doesn't render slot files (`footer.md`) inline
        // — the slot-file flow is exclusively the build pipeline's
        // concern. Pass false here so the editor's heading-state preview
        // for an open `footer.md` shows the would-be H1 the author can
        // toggle off via `title: ""`. The pipeline's authoritative
        // answer still suppresses the H1 because `is_excluded_from_pages`
        // sees the reserved filename.
        slot_only: false,
    })
}

/// Core of `get_file_info`: size, modification time, and (for images) pixel
/// dimensions, for the file viewer's info bars on non-text files.
pub fn get_file_info_inner(file_path: &str) -> Result<EditorFileInfo, String> {
    let meta = std::fs::metadata(file_path)
        .map_err(|e| format!("Failed to read file metadata '{}': {}", file_path, e))?;

    let modified = meta.modified().ok().and_then(|t| {
        let datetime: chrono::DateTime<chrono::Utc> = t.into();
        Some(datetime.to_rfc3339())
    });

    let (width, height) = match std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
    {
        Some(ext)
            if matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tiff" | "avif"
            ) =>
        {
            crate::build::scan::scan::extract_image_dimensions(std::path::Path::new(file_path))
                .map(|(w, h)| (Some(w), Some(h)))
                .unwrap_or((None, None))
        }
        _ => (None, None),
    };

    Ok(EditorFileInfo {
        size: meta.len(),
        modified,
        width,
        height,
    })
}

/// The project-relative spelling of an absolute path, or the shared
/// "not inside project" error. One definition for the five commands that used
/// to hand-roll the same `strip_prefix` + error format.
pub fn project_relative(
    folder_path: &std::path::Path,
    file_path: &str,
) -> Result<String, String> {
    std::path::Path::new(file_path)
        .strip_prefix(folder_path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|_| {
            format!(
                "File '{}' is not inside project '{}'",
                file_path,
                folder_path.display()
            )
        })
}

/// The pure core of `describe_source_role`: given the project root and an
/// absolute file path, classify the source's language + slot role. No `State`,
/// no I/O — so the HTTP carrier arm calls this with the root it resolves from
/// the live serve dir, exactly as the command resolves it from `AppState`.
pub fn describe_source_role_inner(
    folder_path: &std::path::Path,
    file_path: &str,
) -> Result<SourceRole, String> {
    let rel_path = project_relative(folder_path, file_path)?;
    let normalized = moss_core::slug::normalize_separators(&rel_path);
    let normalized = normalized.trim_matches('/');

    let (language, within_tree) = crate::build::footer::language_tree_split(normalized);
    let slot =
        crate::build::footer::slot_from_filename(within_tree).map(|s| s.as_str().to_string());

    let extension = std::path::Path::new(within_tree)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let can_be_page = crate::build::scan::classify::is_page_source(&extension);

    Ok(SourceRole {
        language,
        slot,
        can_be_page,
    })
}

// ── Saving ─────────────────────────────────────────────────────────────────

/// Pure byte-writing core of `save_editor_content`: serialize `frontmatter` +
/// `body` and persist to `file_path`, returning the file's new mtime. Owns NO
/// Tauri types, NO `TaskRegistry`, NO window model — just the same
/// frontmatter-serialize → `fs::write` → read-mtime sequence the Save command
/// has always performed.
///
/// Split out of `save_editor_content_with_task`'s `run_safely` closure so the
/// bytes-to-disk logic has exactly ONE definition, reached by two carriers: the
/// Tauri command (wrapped in `run_safely` + the PanelTask lifecycle) and the
/// HTTP mutation carrier (`preview::server::invoke`). Neither carrier
/// reimplements the write, so they cannot drift — the same reason the read-only
/// arms call `resolve_asset` rather than copy it. The dual-path parity test in
/// `router_tests.rs` pins that both routes leave byte-identical files.
pub fn persist_editor_content(
    file_path: &str,
    frontmatter: &serde_json::Value,
    body: &str,
) -> Result<SaveResult, String> {
    let mut fm_map = super::frontmatter::json_to_yaml_map(frontmatter)?;
    coerce_union_fields(&mut fm_map);
    let content = moss_core::frontmatter::serialize(&fm_map, body)?;
    // The destination is the author's SOURCE file (the one open in the
    // editor), not regenerable output under `.moss/build.nosync/` — its bytes were
    // just read to populate the buffer, so it is materialized.
    // allow:raw_write vault user state, not .moss/build.nosync output
    std::fs::write(file_path, &content)
        .map_err(|e| format!("Failed to write file '{}': {}", file_path, e))?;
    let meta = std::fs::metadata(file_path)
        .map_err(|e| format!("Failed to read mtime for '{}': {}", file_path, e))?;
    let mtime_ms = meta
        .modified()
        .map_err(|e| format!("Failed to read mtime: {}", e))?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Bad mtime: {}", e))?
        .as_millis() as u64;
    Ok(SaveResult { mtime_ms })
}

/// Guard the union frontmatter fields (`children`, `series`) before writing.
///
/// The editor saves the AUTHORED single-key form — a valid `bool`, wikilink
/// `string`, or wikilink `sequence` is written through unchanged (preserving
/// `children: "[[News]]"` byte-for-byte). Only a NON-canonical shape (e.g. a map
/// or number that the widget should never produce, but a malformed paste might)
/// is coerced to the canonical authored form via the shared normalizer, so a
/// junk value can never reach the file. v1 hard-codes the two union field names;
/// schema-driven detection is deferred (see design §4).
fn coerce_union_fields(fm_map: &mut std::collections::HashMap<String, serde_yaml::Value>) {
    use serde_yaml::Value;

    if let Some(v) = fm_map.get("children") {
        let canonical = matches!(v, Value::Bool(_) | Value::String(_));
        if !canonical {
            let norm = moss_core::frontmatter_union::normalize_children(v);
            fm_map.insert("children".to_string(), Value::Bool(norm.children));
        }
    }

    if let Some(v) = fm_map.get("series") {
        let canonical = match v {
            Value::Bool(_) => true,
            Value::Sequence(items) => items.iter().all(|i| matches!(i, Value::String(_))),
            _ => false,
        };
        if !canonical {
            let norm = moss_core::frontmatter_union::normalize_series(v);
            fm_map.insert("series".to_string(), Value::Bool(norm.series));
        }
    }

    // uid is moss-owned metadata and is always semantically a string. YAML may
    // have parsed an all-digit uid as a number (e.g. `uid: 46160604`). Re-quote
    // it on write so the on-disk file self-heals to `uid: "46160604"` — closing
    // the loop on the build-side coercion. Author-owned
    // fields are never silently rewritten; uid is the one field moss owns.
    if let Some(v) = fm_map.get("uid") {
        if matches!(v, Value::Number(_) | Value::Bool(_)) {
            if let Some(s) = moss_core::frontmatter::value_as_string(v) {
                fm_map.insert("uid".to_string(), Value::String(s));
            }
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp directory inside `target/test-tmp/` (project convention).
    /// Never uses `/tmp` or `$HOME` — see MEMORY.md "Test temp folders" rule.
    fn make_test_tmpdir() -> tempfile::TempDir {
        let tmp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/test-tmp");
        std::fs::create_dir_all(&tmp_root).expect("could not create target/test-tmp");
        tempfile::Builder::new()
            .tempdir_in(&tmp_root)
            .expect("could not create temp dir in target/test-tmp")
    }

    /// BUG 3 editor surface: parsing a file with a malformed `---...---` block
    /// (the 'Europe - A Prophecy.md' corruption) must report the YAML error AND
    /// keep the raw block in `body` so the author can repair it in CM6 — no
    /// silent data loss.
    #[test]
    fn parse_frontmatter_preserves_block_and_reports_error() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("europe.md");
        let content =
            "---\nchildren_style: grid\nseries: true\nweight: 10\nuid: blk-europecover: \"006.jpg\"\n---\n\n\nreal body\n";
        std::fs::write(&file, content).expect("write temp md");

        let parsed =
            parse_frontmatter_at(&file.to_string_lossy()).expect("parse_frontmatter");

        assert!(
            parsed.frontmatter_error.is_some(),
            "malformed YAML must surface an error"
        );
        assert!(
            parsed.has_frontmatter,
            "a block exists (even though invalid) → has_frontmatter is true"
        );
        assert!(
            parsed.body.contains("uid: blk-europecover"),
            "raw block must stay in body so CM6 can repair it, got: {}",
            parsed.body
        );
    }

    #[test]
    fn parse_frontmatter_valid_reports_no_error() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("ok.md");
        std::fs::write(&file, "---\ntitle: Hi\n---\nBody.\n").expect("write temp md");

        let parsed =
            parse_frontmatter_at(&file.to_string_lossy()).expect("parse_frontmatter");
        assert!(parsed.frontmatter_error.is_none());
        assert!(parsed.has_frontmatter);
    }

    #[test]
    fn coerce_union_fields_quotes_numeric_uid() {
        // A uid YAML parsed as a number must be re-quoted to a
        // string on write so the on-disk file self-heals.
        let mut m = std::collections::HashMap::new();
        m.insert("uid".to_string(), serde_yaml::Value::Number(46160604u64.into()));
        coerce_union_fields(&mut m);
        assert_eq!(m.get("uid"), Some(&serde_yaml::Value::String("46160604".to_string())));
    }

    #[test]
    fn coerce_union_fields_leaves_string_uid_untouched() {
        let mut m = std::collections::HashMap::new();
        m.insert("uid".to_string(), serde_yaml::Value::String("54ddc5c0".to_string()));
        coerce_union_fields(&mut m);
        assert_eq!(m.get("uid"), Some(&serde_yaml::Value::String("54ddc5c0".to_string())));
    }

    // ── the editor's pending-read seam ────────────────────────────────────

    /// The seam defers on exactly the errors `icloud::is_offline_not_absent`
    /// classifies as offline — no second, parallel notion of "the cloud has it"
    /// — and every message it *does* produce is safe to show a person.
    #[test]
    #[cfg(unix)] // iterates raw libc errnos; the seam itself is platform-shared
    fn the_read_seam_defers_exactly_when_icloud_says_offline_not_absent() {
        let dir = make_test_tmpdir();
        let page = dir.path().join("page.md");
        std::fs::write(&page, "body").unwrap();

        for raw in [libc::EDEADLK, libc::EACCES, libc::EIO, libc::ENOENT] {
            let probe = std::io::Error::from_raw_os_error(raw);
            let offline = crate::build::icloud::is_offline_not_absent(&page, &probe);
            let got = classify_source_read(&page, Err(std::io::Error::from_raw_os_error(raw)));
            assert_eq!(
                matches!(got, Ok(None)),
                offline,
                "errno {raw}: the seam must defer iff icloud says offline-not-absent"
            );
            // Only where the fail-fast policy is actually armed is EDEADLK
            // moss's own doing; elsewhere it is a real error worth reporting.
            #[cfg(target_os = "macos")]
            if let Err(msg) = got {
                let lower = msg.to_lowercase();
                assert!(
                    !lower.contains("deadlock") && !lower.contains("os error 11"),
                    "a user-facing message must never carry the fail-fast errno: {msg}"
                );
            }
        }
    }

    // ── compute_heading_state_inner (JSON-bridge smoke tests) ────────────
    //
    // The full heading rule is unit-tested in `moss_core::heading`. These
    // tests cover the JSON `Value` → `Option<&str>` bridge for `title` and
    // the body-pass-through.

    use moss_core::heading::HeadingSource;

    #[test]
    fn heading_state_bridges_basic_article() {
        let s = compute_heading_state_inner(
            "posts/my-first-post.md",
            &serde_json::json!({}),
            "",
        );
        assert!(s.visible);
        assert_eq!(s.text, "my first post");
        assert!(matches!(s.source, HeadingSource::Filename));
    }

    #[test]
    fn heading_state_bridges_title_field() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": "Custom" }),
            "",
        );
        assert_eq!(s.text, "Custom");
        assert!(matches!(s.source, HeadingSource::Title));
        assert!(s.visible);
    }

    #[test]
    fn heading_state_bridges_empty_title_hides() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": "" }),
            "",
        );
        assert!(!s.visible);
        assert!(matches!(s.source, HeadingSource::Title));
    }

    #[test]
    fn heading_state_bridges_non_string_title_as_filename() {
        // Defensive: only string title:'s are honored. A boolean or number
        // shouldn't accidentally promote to Title source.
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": false }),
            "",
        );
        assert!(s.visible);
        assert!(matches!(s.source, HeadingSource::Filename));
    }

    #[test]
    fn heading_state_bridges_hero_in_body_hides() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({}),
            ":::hero\n# Overlay title\n:::\n\nBody.",
        );
        assert!(!s.visible);
    }

    #[test]
    fn heading_state_keeps_the_pinned_heading_for_an_image_only_hero() {
        // The editor reads the same rule the build does, so the 87-page title
        // loss showed up here too: a full-bleed cover made the pinned heading
        // element vanish from the editor, giving the author no way to see or
        // edit the title of the page they were writing.
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({}),
            ":::hero {image=assets/cover.jpg}\n:::\n\nBody.",
        );
        assert!(s.visible);
    }

    // ── editor_bootstrap helpers ───────────────────────────────────────────

    /// The editor's identity registry is project-root-relative and THROWS on an
    /// absolute path, so a document-mode editor still needs a root even though
    /// there is no site. Emitting none is what left document mode inert: the
    /// panel opened, `EntryRegistry.upsert` threw on the absolute path, and the
    /// document never rendered. Found by running it, not by review.
    #[test]
    fn a_loose_document_roots_the_editor_at_its_parent_directory() {
        assert_eq!(
            bootstrap_editor_root(
                None,
                Some(std::path::Path::new("/home/u/Downloads/notes.md"))
            ),
            Some("/home/u/Downloads".to_string()),
        );
    }

    /// A project always wins — the two fields are the arms of one choice, and
    /// both being set would let the frontend root itself somewhere the site is
    /// not.
    #[test]
    fn a_project_suppresses_the_editor_root() {
        assert_eq!(
            bootstrap_editor_root(
                Some("/home/u/site"),
                Some(std::path::Path::new("/home/u/Downloads/notes.md")),
            ),
            None,
        );
    }

    /// Neither open: the editor still boots, just with no tree (plan D1).
    #[test]
    fn no_project_and_no_document_yields_no_root() {
        assert_eq!(bootstrap_editor_root(None, None), None);
    }

    /// Repo-local temp dir (moss convention: test artifacts live under the
    /// repo's `target/test-tmp/`, never `$HOME`/`/tmp`). Local replica of the
    /// pattern in build.rs's private `repo_temp_dir()` (not importable from
    /// here — it lives inside build.rs's own `mod tests`).
    fn bootstrap_temp_dir() -> tempfile::TempDir {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let test_tmp = manifest_dir
            .parent()
            .map(|repo| repo.join("target").join("test-tmp"))
            .unwrap_or_else(|| manifest_dir.join("target").join("test-tmp"));
        std::fs::create_dir_all(&test_tmp).unwrap();
        // Non-dot prefix: dot-prefixed roots are invisible to `scan_folder`.
        tempfile::Builder::new()
            .prefix("moss-test-")
            .tempdir_in(&test_tmp)
            .unwrap()
    }

    #[test]
    fn resolve_bootstrap_source_absolute_passes_through() {
        // Build an OS-native absolute path: a bare "/abs/..." literal is NOT
        // absolute on Windows (no drive prefix), so it would take the join
        // branch and fail there. Production callers always pass OS-native
        // absolute paths (baked by resolve_page_source).
        let abs = std::env::temp_dir().join("posts").join("a.md");
        let abs_str = abs.to_string_lossy().to_string();
        let resolved = resolve_bootstrap_source(Some("/proj"), &abs_str);
        assert_eq!(resolved.as_deref(), Some(abs_str.as_str()));
        // Absolute source needs no project path at all.
        let resolved = resolve_bootstrap_source(None, &abs_str);
        assert_eq!(resolved.as_deref(), Some(abs_str.as_str()));
    }

    #[test]
    fn resolve_bootstrap_source_relative_joins_project() {
        let resolved = resolve_bootstrap_source(Some("/proj"), "posts/a.md");
        let expected = std::path::Path::new("/proj")
            .join("posts/a.md")
            .to_string_lossy()
            .to_string();
        assert_eq!(resolved.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn resolve_bootstrap_source_relative_without_project_is_none() {
        assert_eq!(resolve_bootstrap_source(None, "posts/a.md"), None);
    }

    #[test]
    fn bootstrap_initial_file_parses_markdown_with_frontmatter() {
        let dir = bootstrap_temp_dir();
        let file = dir.path().join("hello.md");
        std::fs::write(&file, "---\ntitle: Hello\n---\n# Hello body\n").unwrap();
        let parsed = bootstrap_initial_file(
            Some(&dir.path().to_string_lossy()),
            Some(&file.to_string_lossy()),
        )
        .expect("markdown file with frontmatter should parse");
        assert!(parsed.has_frontmatter);
        assert_eq!(
            parsed.frontmatter.get("title"),
            Some(&serde_json::Value::String("Hello".to_string())),
        );
        assert_eq!(parsed.body, "# Hello body\n");
    }

    #[test]
    fn bootstrap_initial_file_relative_source_resolves_against_project() {
        let dir = bootstrap_temp_dir();
        std::fs::create_dir_all(dir.path().join("posts")).unwrap();
        std::fs::write(dir.path().join("posts/a.md"), "plain body\n").unwrap();
        let parsed = bootstrap_initial_file(
            Some(&dir.path().to_string_lossy()),
            Some("posts/a.md"),
        )
        .expect("relative source should resolve via the project path");
        assert!(!parsed.has_frontmatter);
        assert_eq!(parsed.body, "plain body\n");
    }

    #[test]
    fn bootstrap_initial_file_skips_non_markdown() {
        let dir = bootstrap_temp_dir();
        let file = dir.path().join("photo.png");
        std::fs::write(&file, b"not markdown").unwrap();
        assert!(bootstrap_initial_file(
            Some(&dir.path().to_string_lossy()),
            Some(&file.to_string_lossy()),
        )
        .is_none());
    }

    #[test]
    fn bootstrap_initial_file_missing_file_is_none() {
        let dir = bootstrap_temp_dir();
        let file = dir.path().join("absent.md");
        assert!(bootstrap_initial_file(
            Some(&dir.path().to_string_lossy()),
            Some(&file.to_string_lossy()),
        )
        .is_none());
    }

    #[test]
    fn bootstrap_initial_file_none_source_is_none() {
        assert!(bootstrap_initial_file(Some("/proj"), None).is_none());
    }

    #[test]
    fn bootstrap_initial_file_accepts_markdown_extension_case_insensitively() {
        let dir = bootstrap_temp_dir();
        let file = dir.path().join("NOTE.MARKDOWN");
        std::fs::write(&file, "body\n").unwrap();
        let parsed = bootstrap_initial_file(
            Some(&dir.path().to_string_lossy()),
            Some(&file.to_string_lossy()),
        )
        .expect(".MARKDOWN should be accepted case-insensitively");
        assert_eq!(parsed.body, "body\n");
    }

    /// `createRootHomeFile` in editor-main.ts used to compute the root basename in TS
    /// (`projectRoot.replace(/[/\\]+$/,'').split(/[/\\]/).pop()`) to name the site's
    /// home note `<rootName>.md`. That was the 8th basename algorithm and it disagreed
    /// with `moss_core::home::is_home_file` — which elects the home from the REAL root
    /// name — for exactly the inputs below. The bootstrap now emits the answer.
    #[test]
    fn bootstrap_root_name_is_the_resolved_root_name() {
        assert_eq!(
            bootstrap_root_name(Some("/Users/alice/My Blog")).as_deref(),
            Some("My Blog")
        );
        assert_eq!(
            bootstrap_root_name(Some("/Users/alice/My Blog/")).as_deref(),
            Some("My Blog"),
            "a trailing separator must not change the site's home-note name"
        );
        assert_eq!(
            bootstrap_root_name(Some("/Users/alice/My Blog/.")).as_deref(),
            Some("My Blog"),
            "the TS split answered `.` here, which would have created `..md`"
        );
    }

    #[test]
    fn bootstrap_root_name_is_none_without_a_project_or_name() {
        assert_eq!(bootstrap_root_name(None), None);
        assert_eq!(
            bootstrap_root_name(Some("/")),
            None,
            "the filesystem root has no name — the frontend falls back to `index`"
        );
    }
}
