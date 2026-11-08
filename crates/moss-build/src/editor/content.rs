//! Editor source I/O, history retention, and command error containment.
//! Pure parse, save and source policy is re-exported from moss-core.

use super::filesystem::EditorFileInfo;
pub use moss_core::source_edit::{
    SaveResult, ParsedFrontmatter, SourceRole, parse_frontmatter_of,
    bootstrap_source_path, compute_heading_state_inner, describe_source_role_inner,
};

// ── Panic boundary ─────────────────────────────────────────────────────────

/// Run a sync command body with `catch_unwind`. On panic, log the payload and
/// convert to an `Err(String)` returned to the caller.
///
/// This is the containment for the *Tauri wrappers* (and the sync closures
/// they hand it). The HTTP carrier does NOT rely on it per-arm: its
/// containment is one `catch_unwind` at the dispatch point
/// (`preview::server::invoke`), so it holds whether or not an arm's core
/// happens to route through here.
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

/// The event that tells an agent its text was replaced, for a save that kept
/// it: the page's site-relative path, and the history version holding it.
pub fn displaced_event(file_path: &str, result: &SaveResult) -> Option<crate::types::events::MossEvent> {
    let SaveResult::Saved { displaced: Some(version_id), .. } = result else { return None };
    let (_, path) = site_page(file_path)?;
    Some(crate::types::events::MossEvent::OutsideEditKept { path, version_id: version_id.clone() })
}

pub use moss_core::page::content_hash;

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
    use moss_core::source_edit::SourceRead;
    let read = match read {
        Ok(text) => SourceRead::Text(text),
        Err(e) if crate::build::icloud::is_offline_not_absent(path, &e) => {
            log::info!("[editor] {} is still in the cloud", path.display());
            SourceRead::StillArriving
        }
        Err(e) => SourceRead::Failed(e.to_string()),
    };
    moss_core::source_edit::classify_source_read(&path.to_string_lossy(), read)
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

// ── editor_bootstrap cores ─────────────────────────────────────────────────

/// THE root name for the editor bootstrap: resolved once, by the owner.
fn bootstrap_root_name(project_path: Option<&str>) -> Option<String> {
    crate::vault_root::VaultRoot::resolve(project_path?)
        .name_opt()
        .map(str::to_owned)
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

// ── Heading state, file info, source role ──────────────────────────────────

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

// ── Saving ─────────────────────────────────────────────────────────────────

/// Host writer for `save_editor_content`: write `frontmatter` +
/// `body` over `file_path` if the file is still the one the editor read, the
/// bytes whose [`content_hash`] is `expected_hash`. Otherwise nothing is
/// written and the answer is [`SaveResult::Conflict`] with what is on disk: an
/// agent, another editor or a sync client wrote the file, and their text must
/// not be replaced by a buffer that never saw it. Owns no Tauri types and no
/// task or window model.
///
/// One definition reached by two carriers, the Tauri command and the HTTP
/// mutation carrier (`ops::serve::invoke`), so they cannot drift; a parity test
/// in `router_tests.rs` pins that both routes leave byte-identical files.
///
/// With `keep_displaced`, the bytes about to be replaced are another writer's
/// that the person's text wins over: they are kept as a version in the site's
/// history before the write, in this call, so what is kept is exactly what is
/// overwritten. When they cannot be kept nothing is written
/// ([`SaveResult::NotKept`]).
///
/// The file is read, checked and rewritten through one handle, in place: the
/// page keeps its inode, mode, extended attributes and links, and the watcher
/// sees a content change, which keeps the incremental rebuild. A write by
/// another program in the moment between the read and the truncate is still
/// replaced, and nothing notices it afterwards; closing that needs a lock every
/// writer honours, and agents and sync clients do not take ours.
pub fn persist_editor_content(
    file_path: &str,
    frontmatter: &serde_json::Value,
    body: &str,
    expected_hash: &str,
    keep_displaced: bool,
) -> Result<SaveResult, String> {
    use std::io::Read;
    let fail = |e: std::io::Error| format!("Failed to write file '{}': {}", file_path, e);
    // Opened without `create`: renamed or deleted outside moss a moment ago
    // (or its folder was), a save would put the old file back beside the
    // renamed one.
    // allow:raw_write vault user state, not .moss/build.nosync output
    let mut file = match std::fs::OpenOptions::new().read(true).write(true).open(file_path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return match moss_core::source_edit::decide_save(None, Some(expected_hash), frontmatter, body)? {
                moss_core::source_edit::SaveDecision::Gone => Ok(SaveResult::Gone),
                other => Err(format!("A save of the missing file '{}' was not refused: {:?}", file_path, other)),
            };
        }
        Err(e) => return Err(fail(e)),
    };
    let mut on_disk = String::new();
    file.read_to_string(&mut on_disk)
        .map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;
    let content = match moss_core::source_edit::decide_save(Some(on_disk.as_bytes()), Some(expected_hash), frontmatter, body)? {
        moss_core::source_edit::SaveDecision::AlreadySaved => return Ok(SaveResult::Saved {
            hash: content_hash(on_disk.as_bytes()), displaced: None,
        }),
        moss_core::source_edit::SaveDecision::Conflict { disk } => return Ok(SaveResult::Conflict { disk }),
        moss_core::source_edit::SaveDecision::Write { bytes } => bytes,
        moss_core::source_edit::SaveDecision::Gone => return Ok(SaveResult::Gone),
    };
    let displaced = match keep_displaced && content != on_disk.as_bytes() {
        true => match keep_version(file_path, on_disk.as_bytes())? {
            Some(id) => Some(id),
            None => return Ok(SaveResult::NotKept),
        },
        false => None,
    };
    if let Err(e) = rewrite(&mut file, &content) {
        // A full disk can fail the write after the truncate: put back what was
        // there, so the next read is not a torn file taken for an outside edit.
        let _ = rewrite(&mut file, on_disk.as_bytes());
        return Err(fail(e));
    }
    Ok(SaveResult::Saved { hash: content_hash(&content), displaced })
}

/// `bytes`, the page at `file_path` as another writer left it, kept in its
/// site's history; `None` when there is no site or its history is off.
fn keep_version(file_path: &str, bytes: &[u8]) -> Result<Option<String>, String> {
    let Some((site, rel)) = site_page(file_path) else { return Ok(None) };
    crate::deploy::history::HistoryStore::in_vault(site.path())
        .keep_displaced(site.path(), &rel, bytes)
        .map_err(|e| format!("Could not keep the outside edit to '{}' before saving: {}", file_path, e))
}

/// The site holding `file_path`, and the page's path inside it.
fn site_page(file_path: &str) -> Option<(crate::vault_root::VaultRoot, String)> {
    let path = std::path::Path::new(file_path);
    let site = crate::vault_root::VaultRoot::find_containing(path)?;
    let rel = crate::build::scan::path_collision::logical_path(path.strip_prefix(site.path()).ok()?);
    Some((site, rel))
}

fn rewrite(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{Seek, Write};
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(bytes)
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

    /// A save writes over the document that is open, never into the place it
    /// used to be: renamed or deleted outside moss in the moment before an
    /// autosave, it must not re-create the old file beside the renamed one.
    #[test]
    fn a_save_to_a_file_that_is_gone_writes_nothing_and_says_so() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "old").unwrap();
        std::fs::rename(&file, dir.path().join("field-notes.md")).unwrap();

        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "typed", &content_hash(b"old"), false)
            .expect("a vanished file is an outcome, not a write failure");
        assert_eq!(outcome, SaveResult::Gone);
        assert!(!file.exists(), "the old name must not come back");
        assert_eq!(std::fs::read_to_string(dir.path().join("field-notes.md")).unwrap(), "old");
    }

    #[test]
    fn a_save_rewrites_only_the_changed_field_and_keeps_order_quoting_and_comments() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        let before = "---\n# shown on the card\ntitle: \"Harbour\"\nweight: 22\nratio: 1.0\ndate: 2024-01-15\nuid: \"a1b2c3d4\"\n---\nold body\n";
        std::fs::write(&file, before).unwrap();
        let fm = serde_json::json!({ "title": "Harbour", "weight": 23, "ratio": 1, "date": "2024-01-15", "uid": "a1b2c3d4", "stage": "Pier 4" });
        persist_editor_content(&file.to_string_lossy(), &fm, "new body\n", &base(&file), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "---\n# shown on the card\ntitle: \"Harbour\"\nweight: 23\nratio: 1.0\ndate: 2024-01-15\nuid: \"a1b2c3d4\"\nstage: Pier 4\n---\nnew body\n"
        );
    }

    #[test]
    fn a_save_of_a_page_whose_frontmatter_is_broken_writes_the_editor_text() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
        persist_editor_content(&file.to_string_lossy(), &serde_json::json!({ "title": "Fixed" }), "body\n", &base(&file), false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "---\ntitle: Fixed\n---\nbody\n");
    }

    #[test]
    fn a_save_replaces_the_whole_file() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "a much longer body than the one that replaces it").unwrap();
        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "short", &base(&file), false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "short");
        assert_eq!(outcome, SaveResult::Saved { hash: content_hash(b"short"), displaced: None });
    }

    /// The editor read the file, an agent then rewrote it, and the editor's
    /// autosave arrives with text that never saw the agent's change.
    #[test]
    fn a_save_over_a_file_changed_since_it_was_read_writes_nothing_and_returns_the_disk() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "---\ntitle: Harbour\n---\nFirst.\n\nSecond.\n").unwrap();
        let read = base(&file);
        let agent = "---\ntitle: Harbour\n---\nFirst.\n\nSecond, revised by an agent.\n";
        std::fs::write(&file, agent).unwrap();

        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({ "title": "Harbour" }), "First. Typed.\n\nSecond.\n", &read, false).unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), agent, "the agent's edit must survive");
        let SaveResult::Conflict { disk } = outcome else { panic!("expected a conflict, got {outcome:?}") };
        assert_eq!(disk.body, "First.\n\nSecond, revised by an agent.\n");
        assert_eq!(disk.hash, content_hash(agent.as_bytes()), "the disk's hash is the base of the retry");
    }

    /// A browser's save landed but its answer was lost; the retry carries the
    /// old base and finds its own text on disk.
    #[test]
    fn a_retried_save_that_already_landed_is_saved_not_a_conflict() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "---\ntitle: Harbour\n---\nFirst.\n").unwrap();
        let read = base(&file);
        let fm = serde_json::json!({ "title": "Harbour" });
        let first = persist_editor_content(&file.to_string_lossy(), &fm, "First. Typed.\n", &read, false).unwrap();
        let SaveResult::Saved { hash, .. } = first else { panic!("expected the first save to land, got {first:?}") };

        let retry = persist_editor_content(&file.to_string_lossy(), &fm, "First. Typed.\n", &read, false).unwrap();

        assert_eq!(retry, SaveResult::Saved { hash, displaced: None });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "---\ntitle: Harbour\n---\nFirst. Typed.\n");
    }

    /// The same retry after another writer changed the page is still a conflict.
    #[test]
    fn a_retried_save_after_another_writer_changed_the_page_is_a_conflict() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "---\ntitle: Harbour\n---\nFirst.\n").unwrap();
        let read = base(&file);
        let fm = serde_json::json!({ "title": "Harbour" });
        persist_editor_content(&file.to_string_lossy(), &fm, "First. Typed.\n", &read, false).unwrap();
        let agent = "---\ntitle: Harbour\n---\nFirst. Typed, then an agent wrote.\n";
        std::fs::write(&file, agent).unwrap();

        let retry = persist_editor_content(&file.to_string_lossy(), &fm, "First. Typed.\n", &read, false).unwrap();

        assert!(matches!(retry, SaveResult::Conflict { .. }), "got {retry:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), agent, "the agent's text must survive");
    }

    /// A save rewrites the page where it is: the same file, so the watcher
    /// reports a content change (which keeps the incremental rebuild), and a
    /// symlinked page stays a symlink with the mode it had.
    #[cfg(unix)]
    #[test]
    fn a_save_rewrites_the_same_file_and_keeps_its_link_and_mode() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = make_test_tmpdir();
        let real = dir.path().join("real.md");
        std::fs::write(&real, "before").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("note.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let inode = std::fs::metadata(&real).unwrap().ino();

        persist_editor_content(&link.to_string_lossy(), &serde_json::json!({}), "after", &content_hash(b"before"), false).unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "after");
        assert_eq!(std::fs::metadata(&real).unwrap().ino(), inode, "the page is rewritten, not replaced");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);
    }

    /// The bytes on disk and the parse the editor builds its base from agree
    /// on the hash, CRLF included, so a save after opening never conflicts
    /// with itself.
    #[test]
    fn the_parse_names_the_same_base_a_save_checks() {
        let dir = make_test_tmpdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "---\r\ntitle: Harbour\r\n---\r\nbody\r\n").unwrap();
        let parsed = parse_frontmatter_at(&file.to_string_lossy()).unwrap();
        let outcome = persist_editor_content(&file.to_string_lossy(), &parsed.frontmatter, &parsed.body, &parsed.hash, false).unwrap();
        assert!(matches!(outcome, SaveResult::Saved { .. }), "got {outcome:?}");
    }

    fn base(file: &std::path::Path) -> String {
        content_hash(&std::fs::read(file).unwrap())
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

    /// A site folder with one page holding `theirs`, as another writer left it.
    fn site_with_page(theirs: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = make_test_tmpdir();
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        let file = dir.path().join("notes").join("harbour.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, theirs).unwrap();
        (dir, file)
    }

    #[test]
    fn a_save_asked_to_keep_what_it_replaces_keeps_those_exact_bytes_in_history_first() {
        let theirs = "First, as the agent wrote it.\n\nSecond.\n";
        let (dir, file) = site_with_page(theirs);

        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "First, mine.\n\nSecond.\n", &content_hash(theirs.as_bytes()), true).unwrap();

        let SaveResult::Saved { displaced: Some(id), .. } = &outcome else { panic!("expected a kept version, got {outcome:?}") };
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "First, mine.\n\nSecond.\n");
        let history = crate::deploy::history::HistoryStore::in_vault(dir.path());
        assert_eq!(history.read_version(id, "notes/harbour.md").unwrap(), theirs.as_bytes());
        let rows = history.page_timeline("notes/harbour.md");
        assert_eq!(rows.iter().map(|r| r.trigger).collect::<Vec<_>>(), vec![crate::deploy::history::Trigger::Displaced]);
        assert!(matches!(
            displaced_event(&file.to_string_lossy(), &outcome),
            Some(crate::types::events::MossEvent::OutsideEditKept { path, version_id }) if &version_id == id && path == "notes/harbour.md"
        ));
    }

    #[test]
    fn an_outside_edit_to_a_page_the_disk_spells_decomposed_is_kept_under_its_composed_path() {
        let theirs = "Theirs.\n";
        let (dir, _) = site_with_page(theirs);
        let file = dir.path().join("cafe\u{301}").join("menu.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, theirs).unwrap();

        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "Mine.\n", &content_hash(theirs.as_bytes()), true).unwrap();

        assert!(matches!(
            displaced_event(&file.to_string_lossy(), &outcome),
            Some(crate::types::events::MossEvent::OutsideEditKept { path, .. }) if path == "caf\u{e9}/menu.md"
        ), "{outcome:?}");
    }

    #[test]
    fn the_writer_whose_text_was_kept_has_its_next_save_refused_with_the_person_s_text() {
        let theirs = "Agent line.\n";
        let (_dir, file) = site_with_page(theirs);
        persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "Person line.\n", &content_hash(theirs.as_bytes()), true).unwrap();

        let agent = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "Agent line, again.\n", &content_hash(theirs.as_bytes()), false).unwrap();

        let SaveResult::Conflict { disk } = agent else { panic!("the agent's save must be refused, got {agent:?}") };
        assert_eq!(disk.body, "Person line.\n");
    }

    #[test]
    fn a_save_that_cannot_keep_what_it_replaces_writes_nothing() {
        let theirs = "Theirs.\n";
        let (dir, file) = site_with_page(theirs);
        std::fs::write(dir.path().join(".moss/config.toml"), "[history]\nenabled = false\n").unwrap();
        let off = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "Mine.\n", &content_hash(theirs.as_bytes()), true).unwrap();
        assert_eq!(off, SaveResult::NotKept, "history off");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);

        let loose = make_test_tmpdir();
        let file = loose.path().join("loose.md");
        std::fs::write(&file, theirs).unwrap();
        let no_site = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "Mine.\n", &content_hash(theirs.as_bytes()), true).unwrap();
        assert_eq!(no_site, SaveResult::NotKept, "a file in no site");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
    }

    #[test]
    fn an_ordinary_save_keeps_nothing() {
        let (dir, file) = site_with_page("Before.\n");
        let outcome = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "After.\n", &content_hash(b"Before.\n"), false).unwrap();
        assert_eq!(outcome, SaveResult::Saved { hash: content_hash(b"After.\n"), displaced: None });
        assert!(displaced_event(&file.to_string_lossy(), &outcome).is_none());
        assert!(crate::deploy::history::HistoryStore::in_vault(dir.path()).list_records().is_empty());
    }
}

#[cfg(test)]
mod saved_retry_tests {
    use super::*;

    #[test]
    fn stale_retry_of_identical_text_is_saved_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "already there").unwrap();
        let result = persist_editor_content(&file.to_string_lossy(), &serde_json::json!({}), "already there", "old hash", true).unwrap();
        assert_eq!(result, SaveResult::Saved { hash: content_hash(b"already there"), displaced: None });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "already there");
    }
}
