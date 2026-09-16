//! Is this path allowed? — the single path guard for the folder-on-disk domain.
//!
//! Two callers ask that question, and they are not asking the same thing:
//!
//! * **moss's own UI** (the editor, the settings panels) hands us an *absolute*
//!   path the user picked in a file tree. The rule is containment: stay inside
//!   the vault. `.moss/` is fair game — that is where moss keeps its own data,
//!   and moss reading moss's data is the normal case.
//! * **plugins** — third-party code — hand us a *project-relative* string. The
//!   rule is a sandbox: no traversal, and `.moss/` is off limits, because it
//!   holds the identity secret, every plugin's declared capability grants, and
//!   other plugins' stored credentials.
//!
//! Those two rules used to live in two places under one name, and the fence
//! built for the second one silently caged the first: moss's own Analytics
//! dashboard read `.moss/data/events.jsonl` through the plugin door and got a
//! refusal on every load, for eleven days, rendered as a plausible "no data
//! yet" (moss#997). The lesson is not "pick the right function" — it is that a
//! policy repeated at N call sites cannot say *which* door it guards.
//!
//! So the two policies are separate, and the plugin one is a **type**:
//! [`PluginPath`] is the only thing the plugin file commands accept, and
//! [`PluginPath::sandboxed`] is its only constructor. A plugin file operation
//! that skips the sandbox is not a bug you can review for — it does not
//! compile. First-party code calls [`validate_entry_path`], which neither
//! produces nor accepts a `PluginPath`, so it cannot land in the plugin policy
//! by accident.
//!
//! Sibling split worth keeping crisp: this module answers "is this path INSIDE
//! the vault, and may this caller have it?"; `paths.rs` answers "WHICH vault,
//! and what is it called?".

use std::path::{Path, PathBuf};

// ── First-party domain ─────────────────────────────────────────────────────

/// Reject a path component that contains `..` (path-traversal guard).
///
/// Returns `Ok(())` if the path is safe, or an `Err(String)` with a user-facing
/// message if it contains path-traversal syntax.
///
/// This check is performed on the RAW string before any path operations, so it
/// catches traversal attempts even if a later join operation would neutralize them.
pub(crate) fn rejects_traversal(path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err("Path traversal not allowed".to_string());
    }
    Ok(())
}

/// Validate a path moss's own UI supplied, against `project_root`.
///
/// Checks:
/// - `path` must not contain `..` (path-traversal guard).
/// - The resulting [`PathBuf`] must start with `project_root`
///   (project-root containment check).
///
/// Returns the validated [`PathBuf`] on success, or an `Err(String)` with a
/// user-facing message on failure.
///
/// `.moss/` is deliberately **not** fenced off here. This is first-party code
/// reading first-party data; the plugin fence lives on [`PluginPath`].
///
/// # Note on canonicalization
///
/// The `starts_with` check above reads the SPELLING, so on its own it is
/// defeated by a symlinked directory inside the vault that resolves outside
/// it. This function therefore also runs
/// [`recheck_canonical_allowing_missing`], which walks up to the deepest
/// ancestor that exists and re-checks containment on the resolved form —
/// covering both an existing path and a not-yet-created destination, so
/// callers get one guard rather than a lexical one they must remember to
/// pair.
///
/// Pairing it by hand is exactly what went wrong: nine call sites, and
/// `create_entry_path` was the one that never did, which left `create_file`
/// and `create_folder` able to write outside the vault on BOTH carriers.
/// [`recheck_canonical`] remains public for the callers that need the
/// resolved `(root, target)` pair back, not for containment.
pub fn validate_entry_path(project_root: &Path, path: &str) -> Result<PathBuf, String> {
    rejects_traversal(path)?;
    let p = PathBuf::from(path);
    if !p.starts_with(project_root) {
        return Err("Paths must be within the project directory".to_string());
    }
    recheck_canonical_allowing_missing(project_root, &p)?;
    Ok(p)
}

/// The canonical-form recheck that [`validate_entry_path`]'s doc prescribes:
/// canonicalize both sides and repeat the containment check, defeating
/// symlink-escape (a symlinked component inside the vault pointing outside
/// it). `target` must exist; for a not-yet-existing destination, pass its
/// parent. Returns `(root_canonical, target_canonical)` so callers can add
/// their own root-equality checks without re-resolving.
pub fn recheck_canonical(
    project_root: &Path,
    target: &Path,
) -> Result<(PathBuf, PathBuf), String> {
    let root_canonical = std::fs::canonicalize(project_root)
        .map_err(|e| format!("Failed to resolve project root: {}", e))?;
    let target_canonical = std::fs::canonicalize(target)
        .map_err(|e| format!("Failed to resolve '{}': {}", target.display(), e))?;
    if !target_canonical.starts_with(&root_canonical) {
        return Err(format!(
            "'{}' must be within the project directory",
            target.display()
        ));
    }
    Ok((root_canonical, target_canonical))
}

/// The containment recheck for a destination that does not exist yet.
///
/// [`recheck_canonical`] cannot resolve a path that is not on disk, so a
/// caller creating a new file has nothing to hand it. Walking up to the
/// deepest ancestor that DOES exist and rechecking that is sound: a component
/// that does not exist cannot be a symlink, so once the deepest existing
/// ancestor resolves inside the root, no suffix of not-yet-created components
/// can leave it.
///
/// "Exists" here means `symlink_metadata`, NOT `Path::exists` — the latter
/// FOLLOWS symlinks, so a dangling one reads as absent and the walk-up steps
/// straight over the one component that could redirect the write. A dangling
/// link is then handed to [`recheck_canonical`], whose `canonicalize` fails
/// and refuses it. Broken symlinks are ordinary: git checks them out, and a
/// link to an unmounted volume is one.
///
/// Without this, a symlinked directory inside the vault is a write primitive
/// pointing anywhere — `validate_entry_path` is lexical and sees only the
/// spelling, which still starts with the root.
pub fn recheck_canonical_allowing_missing(
    project_root: &Path,
    target: &Path,
) -> Result<(), String> {
    let mut probe = target;
    loop {
        if probe.symlink_metadata().is_ok() {
            recheck_canonical(project_root, probe)?;
            return Ok(());
        }
        match probe.parent() {
            Some(parent) if parent != probe && !parent.as_os_str().is_empty() => probe = parent,
            _ => {
                return Err(format!(
                    "'{}' must be within the project directory",
                    target.display()
                ))
            }
        }
    }
}

/// Move a file or folder to the OS trash — recoverable via the file manager,
/// never a permanent unlink. Refuses to trash the project root itself (callers
/// should never surface Delete on the root; defend in depth). The ONE delete
/// core: the desktop `delete_entry` command and the HTTP mutation arm both
/// call here, so the two carriers cannot drift on the containment checks.
pub fn delete_entry_inner(project_root: &Path, path: &str) -> Result<(), String> {
    // Fast pre-check: traversal guard + raw starts_with.
    let target = validate_entry_path(project_root, path)?;

    // Already gone — the state the delete asked for. Without this, the
    // canonical recheck below turns an achieved goal into "Failed to
    // resolve … os error 2": on 2026-09-05 a Drive-synced vault trashed
    // `untitled.md` moments before the command landed, and the author got a
    // failure dialog for a delete that had succeeded.
    if target.symlink_metadata().is_err() {
        return Ok(());
    }

    // Canonical-form recheck to defeat symlink-escape.
    let (root_canonical, target_canonical) = recheck_canonical(project_root, &target)?;

    if target_canonical == root_canonical {
        return Err("Cannot delete the project root".to_string());
    }

    // On macOS, trash via `NSFileManager.trashItemAtURL`, not the crate's
    // default Finder AppleScript. The Finder route serializes through a
    // busy Finder under the default ~60s AppleEvent ceiling — on a cloud
    // File Provider vault it timed out (-1712) where trashItemAtURL took
    // 30ms (measured 2026-09-05, Google Drive) — and it needs the
    // Automation permission whose denial (-1743) was moss#1171. Both
    // failure modes cease to exist on this route. Known cost: Finder may
    // not offer "Put Back" for items trashed this way; drag-out recovery
    // still works.
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    match ctx.delete(&target) {
        Ok(()) => Ok(()),
        // Vanished mid-flight (the pre-check's race window): goal state
        // reached, same as the pre-check.
        Err(_) if target.symlink_metadata().is_err() => Ok(()),
        Err(e) => Err(format!("Failed to move '{}' to trash: {}", path, e)),
    }
}

/// Rename an entry: path-traversal guard + project-root boundary check +
/// `fs::rename`, then carry a folder's self-named home file to the new name.
/// The rename door beside the create and delete doors above; the app's
/// `rename_entry` command and `editor::ref_scan`'s rename-with-refs both
/// call here.
///
/// This crosses the REAL FS boundary (no mocks). The TS-side tests mock
/// `renameEntry` and assert on its relative-path argument, which baked in the
/// origin bug (a relative `old`/`new` slipped past a boundary check that was
/// only ever exercised through a mock). The cross-boundary regression lives in
/// `tests/rename_boundary_test.rs` and calls this fn directly (#715/#731).
///
/// `old`/`new` must be ABSOLUTE paths under `project_root` — the same contract
/// the command receives from the frontend (which resolves to absolute at the FS
/// boundary). A relative path will fail the `starts_with` check and is rejected
/// with the project-directory error, never reaching `fs::rename`.
pub fn rename_entry_inner(
    project_root: &std::path::Path,
    old: &str,
    new: &str,
) -> Result<(), String> {
    let old_p = validate_entry_path(project_root, old)?;
    let new_p = validate_entry_path(project_root, new)?;

    // Canonical-form recheck to defeat symlink-escape, same strength as the
    // copy/move/delete doors: the source itself, and — since the destination
    // does not exist yet — the destination's parent.
    recheck_canonical(project_root, &old_p)?;
    let new_parent = new_p
        .parent()
        .ok_or_else(|| "Invalid destination path".to_string())?;
    recheck_canonical(project_root, new_parent)?;

    // Detect whether this is a directory rename before we move it.
    let is_dir = old_p.is_dir();

    std::fs::rename(&old_p, &new_p)
        .map_err(|e| format!("Failed to rename '{}': {}", old_p.display(), e))?;

    // After a successful directory rename, check whether the folder had a
    // self-named home file and carry it to the new name (best-effort).
    if is_dir {
        rename_self_named_home(project_root, &old_p, &new_p);
    }

    Ok(())
}

/// After a directory is renamed from `old_dir` to `new_dir`, rename its
/// self-named home file if one exists.
///
/// A "self-named" home file is `<old_folder_name>.md` (case-insensitive, with
/// optional recognized lang-suffix) inside the directory.  Index/README/
/// `_index`/`main` stems are left untouched — they are conventional and do not
/// need to track the folder name.
///
/// This is best-effort: any error emits a `log::warn!` and returns without
/// propagating — the `home: true` marker keeps resolution working even if the
/// file name doesn't match.
fn rename_self_named_home(
    project_root: &std::path::Path,
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
) {
    // Extract old and new folder base-names as lowercase strings.
    let old_name = match old_dir.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return,
    };
    let new_name = match new_dir.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return,
    };

    // Build the candidate path: <new_dir>/<old_name>.md
    // (the file was already moved with the directory)
    let candidate = new_dir.join(format!("{}.md", old_name));
    if !candidate.exists() {
        // No self-named home file found — nothing to carry.
        return;
    }

    // Extract the stem of the candidate (without ".md") to check whether it
    // is truly self-named for the OLD folder and not an index stem.
    let stem = match candidate
        .file_stem()
        .and_then(|s| s.to_str())
    {
        Some(s) => s.to_string(),
        None => return,
    };

    // Strip a recognized lang-suffix if present (e.g. "游记.zh-hans" → "游记").
    let bare_stem = moss_core::home::strip_lang_suffix(&stem)
        .unwrap_or(&stem)
        .to_string();

    // If the bare stem is a recognized index stem (index, readme, etc.),
    // do NOT rename — those are conventional and folder-name-independent.
    if moss_core::home::is_index_stem(&bare_stem) {
        return;
    }

    // Confirm the bare stem matches the OLD folder name (case-insensitive).
    if bare_stem.to_lowercase() != old_name.to_lowercase() {
        // Not self-named for this folder — leave it alone.
        return;
    }

    // Build the destination: <new_dir>/<new_name>.md (preserving any lang suffix).
    let new_filename = match stem.rsplit_once('.') {
        // A recognized lang suffix was stripped — preserve it.
        Some((_, suffix)) if bare_stem != stem => format!("{new_name}.{suffix}.md"),
        // No lang suffix — plain `<new_name>.md`
        _ => format!("{new_name}.md"),
    };

    let destination = new_dir.join(&new_filename);

    // Validate destination is still within the project root (best-effort guard).
    if let Err(e) = validate_entry_path(project_root, destination.to_str().unwrap_or("")) {
        log::warn!(
            "rename_self_named_home: destination '{}' failed path guard: {}",
            destination.display(),
            e
        );
        return;
    }

    if let Err(e) = std::fs::rename(&candidate, &destination) {
        log::warn!(
            "rename_self_named_home: could not rename '{}' → '{}': {}",
            candidate.display(),
            destination.display(),
            e
        );
    }
}

// ── Entry creation (create_file / create_folder guards) ────────────────────

/// Shared name guard for `create_file_inner` / `create_folder_inner`. The name
/// is a user-typed BASENAME, never a path: separators and traversal are
/// rejected rather than silently joined ("../x" would otherwise create outside
/// the chosen dir), an embedded NUL is rejected (Unix `open(2)` truncates the
/// path at the first NUL, so the created file would differ from the validated
/// string), and trailing dots/spaces are trimmed (unrepresentable on Windows
/// shares; also collapses "." / ".." / "..." to the empty-name error).
pub(crate) fn sanitize_entry_name(raw: &str) -> Result<String, String> {
    let name = raw.trim().trim_end_matches(['.', ' ']);
    if name.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if name.contains('\0') {
        return Err("Name contains an invalid character".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain path separators".to_string());
    }
    Ok(name.to_string())
}

/// Page creation implies markdown: append ".md" when the typed name carries no
/// recognizable extension. A suffix counts as an extension only when it is
/// short, ASCII-alphanumeric and letter-led — "v1.2" is a version string, not
/// a file with extension "2", so it becomes "v1.2.md"; "notes.txt" already
/// carries a deliberate extension and is left alone.
fn ensure_page_extension(name: String) -> String {
    let is_extension = name.rsplit_once('.').is_some_and(|(_, ext)| {
        (1..=10).contains(&ext.len())
            && ext.as_bytes()[0].is_ascii_alphabetic()
            && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    if is_extension { name } else { format!("{name}.md") }
}

/// Containment-check the parent dir against the project root, join the
/// sanitized name, and enforce the no-overwrite rule. The "already exists"
/// wording is load-bearing: the file-tree keys its localized collision
/// message off it.
fn create_entry_path(
    project_root: &Path,
    dir: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let dir_p = validate_entry_path(project_root, dir)?;
    let path = dir_p.join(name);
    if path.exists() {
        return Err(format!("'{}' already exists", path.display()));
    }
    Ok(path)
}

/// Create one empty note. Monotonic mode: the filename IS the title (see
/// docs/archive/2026-05-25-editor-heading-monotonic.md), so a new file starts
/// with no frontmatter and no `title:`; authors add fields through the ChipBar,
/// which writes them on first edit.
pub fn create_file_inner(
    project_root: &Path,
    dir: &str,
    name: &str,
) -> Result<String, String> {
    create_file_with_content_inner(project_root, dir, name, "")
}

/// [`create_file_inner`] with the file's first bytes. One write, not
/// create-empty-then-overwrite: the watcher turns each write into a structural
/// rebuild, and the empty first version lands in the build's source baseline
/// as a zero-length file that the next drift sweep then reports as edited —
/// three full rebuilds for one new page (2026-09-05 log, template instantiation).
pub fn create_file_with_content_inner(
    project_root: &Path,
    dir: &str,
    name: &str,
    content: &str,
) -> Result<String, String> {
    let path = plan_new_page(project_root, dir, name)?;
    write_new_page(&path, content)
}

/// One note to create: `dir` is absolute, `name` carries the extension,
/// `frontmatter` is the JSON object the editor's save path already speaks.
/// The body is always empty: the filename is the title (monotonic mode).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct NewFile {
    pub dir: String,
    pub name: String,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
}

/// Guarded core of the `create_files` command: every note in `files` is
/// validated before any is written, so a takeover that scaffolds two files
/// (a folder home plus a claim page) either lands whole or not at all short
/// of an I/O failure mid-way. Returns the absolute paths, in order.
pub fn create_files_inner(project_root: &Path, files: &[NewFile]) -> Result<Vec<String>, String> {
    let mut planned: Vec<(PathBuf, String)> = Vec::with_capacity(files.len());
    for f in files {
        let path = plan_new_page(project_root, &f.dir, &f.name)?;
        if planned.iter().any(|(p, _)| *p == path) {
            return Err(format!("'{}' appears twice in one batch", path.display()));
        }
        let content = if f.frontmatter.is_empty() {
            String::new()
        } else {
            let map = crate::editor::frontmatter::json_to_yaml_map(&serde_json::Value::Object(f.frontmatter.clone()))?;
            moss_core::frontmatter::serialize(&map, "")?
        };
        planned.push((path, content));
    }
    planned.iter().map(|(path, content)| write_new_page(path, content)).collect()
}

/// Sanitize the name, imply `.md`, containment-check the parent and enforce
/// the no-overwrite rule — everything that can fail before any byte lands.
fn plan_new_page(project_root: &Path, dir: &str, name: &str) -> Result<PathBuf, String> {
    let name = ensure_page_extension(sanitize_entry_name(name)?);
    create_entry_path(project_root, dir, &name)
}

/// The one writer behind every new-page path. Missing parent directories are
/// created: the containment check already admits a missing parent, and the
/// first claim in a namespace is exactly the case where the folder does not
/// exist yet.
fn write_new_page(path: &Path, content: &str) -> Result<String, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create folder '{}': {}", parent.display(), e))?;
    }
    // allow:raw_write creates a brand-new note in the vault content tree — never .moss/build/, and the path was just uniqueness-checked
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to create file '{}': {}", path.display(), e))?;
    Ok(path.to_string_lossy().to_string())
}

/// Guarded core of the `create_folder` command (no ".md" enforcement).
pub fn create_folder_inner(
    project_root: &Path,
    parent_dir: &str,
    name: &str,
) -> Result<String, String> {
    let name = sanitize_entry_name(name)?;
    let path = create_entry_path(project_root, parent_dir, &name)?;
    std::fs::create_dir(&path)
        .map_err(|e| format!("Failed to create folder '{}': {}", path.display(), e))?;
    Ok(path.to_string_lossy().to_string())
}

// ── Plugin domain ──────────────────────────────────────────────────────────

/// A project-relative path that has passed the **plugin** sandbox policy.
///
/// Every plugin file command takes one of these rather than a `&str`, and
/// [`PluginPath::sandboxed`] is the only way to make one. That is what makes
/// the policy apply once instead of being hand-copied per command — and what
/// makes "a plugin file command that forgot the fence" unwritable rather than
/// merely unreviewed.
///
/// First-party code has no reason to construct one; if you are reaching for
/// this from moss's own UI, the file you want has an owning module that should
/// read it for you (see moss#997).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPath(String);

impl PluginPath {
    /// The single plugin door: apply the sandbox policy to a plugin-supplied
    /// project-relative path.
    ///
    /// Rejects, in order:
    /// - the empty string,
    /// - absolute paths,
    /// - any `..` segment (directory traversal),
    /// - an embedded NUL — on Unix `open(2)` truncates a path at the first NUL,
    ///   so `image\0.jpg` would read a different file than the validated string,
    /// - anything whose first segment is `.moss` (case-insensitively).
    ///
    /// That last rule is the credential fence. `.moss/` holds the identity
    /// secret (`identity/secret-key`), every plugin's installed manifest —
    /// which is where declared capability grants are read from — and other
    /// plugins' stored credentials. A plugin that could read it could lift the
    /// signing key; one that could write it could grant itself capabilities it
    /// never declared. Plugins reach their own storage through
    /// [`PluginPath::storage`] and build output through `read_site_file`.
    pub fn sandboxed(relative_path: &str) -> Result<Self, String> {
        relative_segment_rules(relative_path)?;
        if first_segment_is_moss(relative_path) {
            return Err(
                "Access to .moss/ is not allowed. Use plugin storage (readPluginFile / writePluginFile) or readSiteFile instead."
                    .to_string(),
            );
        }
        Ok(Self(relative_path.to_string()))
    }

    /// The plugin door for a plugin's **own** storage, under its installed
    /// directory `.moss/plugins/<plugin_name>/` — the directory root, not a
    /// `data/` subfolder, which is why the two names refused below are refused
    /// here rather than somewhere deeper.
    ///
    /// A different policy from [`PluginPath::sandboxed`], deliberately: here
    /// `.moss/` is the destination rather than the thing being fenced off, so
    /// the fence is replaced by two narrower rules — `plugin_name` must be a
    /// single path segment (no escaping into a sibling plugin's storage), and
    /// the leaf must be neither `manifest.json`, because the manifest is where
    /// the plugin's capability grants are read from and a writable manifest is
    /// a self-service capability escalation, nor `.moss-install.json`, for the
    /// reason given at that check. There is deliberately no host function that
    /// deletes a plugin file — unlinking either of these would defeat the
    /// check as thoroughly as writing it.
    ///
    /// Returns the validated path *relative to the plugin's own directory*;
    /// resolve it with [`PluginPath::resolve_under`] against that directory.
    pub fn storage(plugin_name: &str, relative_path: &str) -> Result<Self, String> {
        if plugin_name.is_empty()
            || plugin_name.contains('/')
            || plugin_name.contains('\\')
            || plugin_name.contains("..")
            || plugin_name.contains('\0')
        {
            return Err("Invalid plugin name: must be a single path segment".to_string());
        }
        // First, so `storage("ipfs", "../../manifest.json")` is reported as
        // the traversal it is rather than as the named file it was aimed at.
        relative_segment_rules(relative_path)?;
        let leaf = relative_path
            .replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or(relative_path)
            .to_string();
        // Refused before the two names below are looked for, because Windows
        // opens several spellings as the same file and a check that normalized
        // and then compared would lose to whichever spelling it had not heard
        // of. `manifest.json.` (trailing dots and spaces are stripped) and
        // `manifest.json::$DATA` (the default NTFS data stream) both open the
        // manifest. So a name that is not already its own canonical form is
        // not a name: nothing legitimate ends in a dot or a space, and Windows
        // forbids `:` outright — the same argument `is_rooted` makes.
        //
        // Scoped to the leaf, and to this door, rather than moved into
        // `relative_segment_rules` where both doors would get it: what it
        // protects is the two fixed names below, and the vault proper holds
        // real files a blanket `:` rule would refuse (`1:x.md`, asserted in
        // `ordinary_project_paths_are_still_allowed`). A `data:x/` directory
        // under a plugin's own storage buys an attacker nothing — an NTFS
        // stream name cannot have children, so the write simply fails.
        //
        // Known limit: on a volume with 8.3 aliases enabled, `MANIFE~1.JSO`
        // still reaches the file. Refusing `~` would cost real filenames.
        if leaf.contains(':') || leaf != leaf.trim_end_matches(['.', ' ']) {
            return Err(
                "Invalid file name: trailing dots or spaces and ':' are not portable".to_string(),
            );
        }
        if leaf.eq_ignore_ascii_case("manifest.json") {
            return Err(
                "Plugins cannot write manifest.json — it declares their capability grants"
                    .to_string(),
            );
        }
        // Same reason, one file over: `.moss-install.json` is moss's record of
        // which paths an install laid down, and an update reads it to tell the
        // last version's code from the plugin's own files. A plugin that could
        // write it could empty the list and have code a new version deleted
        // carried forward as if the plugin had written it.
        if leaf.eq_ignore_ascii_case(".moss-install.json") {
            return Err(
                "Plugins cannot write .moss-install.json — it records what their install laid down"
                    .to_string(),
            );
        }
        Ok(Self(relative_path.to_string()))
    }

    /// Join this validated path onto a base directory.
    ///
    /// The only way to turn a `PluginPath` into something openable, so every
    /// plugin file operation passes through the policy above.
    pub fn resolve_under(&self, base: &Path) -> PathBuf {
        base.join(&self.0)
    }

    /// The validated relative path, for log lines and error messages.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Shared primitive ───────────────────────────────────────────────────────

/// Whether a path escapes whatever directory it is about to be joined under.
///
/// One owner, in the pure crate, for both trust boundaries that ask this: the
/// plugin-path rules below and `[editor].attachment_folder`. The rationale for
/// not using `Path::is_absolute()` travels with the function.
pub use moss_core::attachment::is_rooted;

/// The rules every plugin-supplied relative path obeys, whichever door it
/// came through: non-empty, relative, no `..`, no NUL.
fn relative_segment_rules(relative_path: &str) -> Result<(), String> {
    if relative_path.is_empty() {
        return Err("Relative path cannot be empty".to_string());
    }
    if is_rooted(relative_path) {
        return Err("Absolute paths are not allowed".to_string());
    }
    if relative_path.contains("..") {
        return Err("Invalid path: directory traversal not allowed".to_string());
    }
    if relative_path.contains('\0') {
        return Err("Invalid path: NUL byte not allowed".to_string());
    }
    Ok(())
}

/// Whether the first meaningful segment of a project-relative path is `.moss`.
///
/// Skips empty and `.` segments so `./.moss/x` and `.moss/x` land the same
/// way, and compares case-insensitively so `.MOSS/x` does too (macOS and
/// Windows both have case-insensitive filesystems in the default setup).
fn first_segment_is_moss(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    normalized
        .split('/')
        .find(|seg| !seg.is_empty() && *seg != ".")
        .is_some_and(|first| first.eq_ignore_ascii_case(".moss"))
}

#[cfg(test)]
#[path = "fs_tests.rs"]
mod tests;
