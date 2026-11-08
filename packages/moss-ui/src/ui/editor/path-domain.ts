// Pure helpers — no imports from tauri/bindings. Bridge the
// project-root-relative path domain (which EntryRegistry stores
// internally)
// and the absolute-path domain used by FileTree's TreeNode.path
// ingestion (per crates/moss-build/src/editor/filesystem.rs:list_directory_inner)
// and Tauri command call sites (Rust opens files via std::path).
//
// Convention (matches Zed Entry.path = Arc<RelPath>, VS Code Uri.path
// vs Uri.fsPath, Obsidian TFile.path): relative-everywhere internal
// model, explicit toAbsolute() at the FS boundary.
//
// The root of the project is represented by the empty string PROJECT_ROOT_REL.

/**
 * The project-root-relative representation of the project root itself.
 *
 * `toRelative(projectRoot, projectRoot)` returns this. Use it for
 * grep-able presence rather than bare `''` literals throughout the
 * codebase.
 */
export const PROJECT_ROOT_REL = '';

/**
 * The COMPARISON form of a path: one Unicode spelling (NFC), for asking
 * whether two paths name the same thing.
 *
 * `é` can be written as one code point or as `e` + a combining accent. The two
 * are the same filename to a person and to macOS, and different strings to
 * `===`, `startsWith` and `Map.has`. A keyboard emits the precomposed form; a
 * macOS directory listing emits the decomposed one. Comparing them raw
 * silently loses state — an expansion override keyed on what the user typed
 * stops matching the path the next listing reports, so the refresh prunes it
 * and the folder the user just renamed collapses.
 *
 * THE RULE: paths are STORED exactly as the filesystem spells them, and
 * normalized only where two of them are compared. Normalizing at ingest was
 * tried first and is wrong: every filesystem call derives from the stored
 * path, and Linux filesystems are normalization-SENSITIVE, so a rewritten path
 * would display a file that then cannot be opened, renamed or deleted. macOS
 * tolerates either spelling on lookup; ext4 does not. Bytes go to disk
 * untouched; equality is asked in this form.
 *
 * CJK is unaffected either way — Han ideographs have no canonical
 * decomposition, which is also why a CJK test case cannot prove any of this.
 */
export function normalizePath(path: string): string {
  return path.normalize('NFC');
}
//
// Both functions are idempotent: they detect the input's domain by
// inspecting the leading `/`. That is deliberate — callers in
// editor-main.ts receive paths from a wire whose domain shifts (the
// `editor:file-renamed` event carries absolute, the `file-changed`
// event carries relative). Idempotence lets callers always call the
// helper without per-event branching.

/**
 * A Windows absolute path (`C:\...` or `C:/...`) has no leading `/`, so a
 * POSIX-only `startsWith('/')` check reads it as already-relative. On
 * Windows every `sourcePath` the Rust side hands back is drive-letter form
 * (`resolve_page_source_inner` / `open_editor`'s warm-open node both carry
 * it that way), so that misdetection fired on every real article: toRelative
 * passed the absolute path through unchanged (it "isn't absolute"), and the
 * caller's later toAbsolute call then read that same string as relative and
 * prefixed projectRoot onto it AGAIN — `<root>/<root>\<file>`, a path that
 * doesn't exist — which is why `parseFrontmatter` failed and the editor
 * panel silently kept showing the pre-warmed "generated" (no editable page)
 * surface instead of the article.
 */
const WINDOWS_DRIVE_ABS_RE = /^[A-Za-z]:[\\/]/;

export function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || WINDOWS_DRIVE_ABS_RE.test(path);
}

/**
 * Convert a project-root-relative path to an absolute path.
 *
 * If `relOrAbs` is already absolute (leading `/`, or a Windows drive letter
 * like `C:\`), it is returned unchanged.
 *
 * `projectRoot` is normalized to drop any trailing slash.
 */
export function toAbsolute(relOrAbs: string, projectRoot: string): string {
  if (isAbsolutePath(relOrAbs)) return relOrAbs;
  if (!projectRoot) return relOrAbs; // defensive: empty root would yield '/relOrAbs'
  const root = projectRoot.endsWith('/') ? projectRoot.slice(0, -1) : projectRoot;
  return `${root}/${relOrAbs}`;
}

/**
 * Convert an absolute path to a project-root-relative path.
 *
 * Symmetric idempotence with `toAbsolute`: relative inputs pass through.
 * Absolute inputs outside `projectRoot` (e.g. `/etc/hosts`) also pass
 * through, but emit a `console.warn` — pass-through preserves rename
 * pipeline resilience while still surfacing the misuse for debugging.
 *
 * `projectRoot` is normalized to drop any trailing slash. The separator
 * right after the stripped root may be `/` or `\`: `projectRoot` and
 * `absOrRel` both come from the same Rust `PathBuf` Display on Windows, so
 * they agree with each other, but that shared separator is platform-native
 * rather than forced to `/`.
 */
export function toRelative(absOrRel: string, projectRoot: string): string {
  if (!isAbsolutePath(absOrRel)) return absOrRel;
  if (!projectRoot) return absOrRel; // defensive: no root to strip
  const root = projectRoot.endsWith('/') || projectRoot.endsWith('\\')
    ? projectRoot.slice(0, -1)
    : projectRoot;
  // The project root itself maps to PROJECT_ROOT_REL.
  if (absOrRel === root) return PROJECT_ROOT_REL;
  const sepAfterRoot = absOrRel.startsWith(root) ? absOrRel.charAt(root.length) : '';
  if (sepAfterRoot !== '/' && sepAfterRoot !== '\\') {
    console.warn(
      `[path-domain] toRelative: '${absOrRel}' is outside projectRoot '${root}' — passing through`,
    );
    return absOrRel;
  }
  return absOrRel.slice(root.length + 1);
}

/**
 * Return the path of a sibling of `path` with its basename replaced by
 * `newBasename`, preserving the parent directory. Project-root-relative in,
 * project-root-relative out.
 *
 * Single source of truth for "rename a file in place" path math, shared by
 * the file-tree inline rename (`FileTree.startRename`) and the editor-heading
 * rename (`routeHeadingCommit`). Critically, a ROOT-LEVEL file (no slash)
 * returns just `newBasename` with NO leading slash — the naive
 * `parentDir + "/" + name` form produced "/name.md", which `toAbsolute`
 * treats as already-absolute and leaves outside the project, tripping
 * rename_entry's "Paths must be within the project directory" check.
 */
export function siblingPath(path: string, newBasename: string): string {
  const lastSlash = path.lastIndexOf('/');
  return lastSlash >= 0 ? `${path.slice(0, lastSlash)}/${newBasename}` : newBasename;
}

/**
 * Where a create lands, given the tree's breadcrumb anchor: the anchor itself
 * when it is a directory, else the anchor's parent directory. Pure query over
 * (path, isDir) — no DOM, no controller state — so the planned model
 * extraction can lift
 * it unchanged. Project-root-relative in, project-root-relative out; a
 * root-level file anchor resolves to PROJECT_ROOT_REL.
 */
export function resolveCreateTarget(anchorPath: string, anchorIsDir: boolean): string {
  return anchorIsDir ? anchorPath : parentDirOf(anchorPath);
}

/**
 * The directory holding `path`. Project-root-relative in, project-root-
 * relative out; a root-level entry resolves to PROJECT_ROOT_REL (the empty
 * string), which is the same domain `childPath` speaks.
 */
export function parentDirOf(path: string): string {
  const lastSlash = path.lastIndexOf('/');
  return lastSlash >= 0 ? path.slice(0, lastSlash) : PROJECT_ROOT_REL;
}

/** The last segment of `path` — the name a row displays. The root's is ''. */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

/**
 * Replace every occurrence of a known absolute path in `message` with its
 * basename. For a Rust filesystem error that echoes back the exact path it
 * was given (e.g. a trash failure naming the file it couldn't move) — a
 * ~120-character iCloud/Drive path is noise once the message already says
 * what went wrong; the filename is what identifies the file to the person
 * reading the toast.
 *
 * Only paths WE supplied are substituted — this is not a match against the
 * OS's own (localized, unstable) wording, just literal replacement of
 * strings this call already knows it sent across the Tauri boundary.
 */
export function shortenKnownPaths(message: string, absPaths: readonly string[]): string {
  return absPaths.reduce(
    (msg, path) => (path ? msg.split(path).join(baseName(path)) : msg),
    message,
  );
}

/**
 * The name a row, a breadcrumb or a menu shows for `path`: its last segment,
 * or for the project root (PROJECT_ROOT_REL, '') the folder's name on disk —
 * so a breadcrumb reads 'my-blog' rather than nothing.
 */
export function displayName(path: string, projectRoot: string): string {
  if (path === PROJECT_ROOT_REL) {
    const root = projectRoot.replace(/\/$/, '');
    return root.split('/').pop() ?? root;
  }
  return path.split('/').pop() ?? path;
}

/**
 * Split a filename into a display stem and a dim extension hint.
 * Markdown files drop their extension entirely; other types show a dim suffix.
 * Dotfiles (e.g. `.gitignore`) and extensionless names render as-is.
 */
export function fileDisplayParts(name: string): { stem: string; extHint: string } {
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return { stem: name, extHint: '' };
  const ext = name.slice(dot);
  if (ext.toLowerCase() === '.md') return { stem: name.slice(0, dot), extHint: '' };
  return { stem: name.slice(0, dot), extHint: ext };
}

/**
 * Return the path of `basename` as a direct child of `dirPath`.
 * Project-root-relative in, project-root-relative out.
 *
 * Root-aware for the same reason as `siblingPath`: the project root is the
 * EMPTY string, so the naive `dirPath + "/" + basename` mints "/basename" —
 * a leading-slash path outside this domain, which `toAbsolute` treats as
 * already absolute. The create flow (`FileTree.startCreate`) re-introduced
 * that exact bug at the root after rename had already been fixed; this is
 * the shared join both temp-entry sites use.
 */
export function childPath(dirPath: string, basename: string): string {
  return dirPath === PROJECT_ROOT_REL ? basename : `${dirPath}/${basename}`;
}

/**
 * The root-relative path of absolute `abs` when it sits inside absolute
 * `root` (the file is already in the folder), else null.
 *
 * Both sides are NFC-normalized first: a path from a native OS dialog comes
 * from outside moss's normalized channels, and macOS hands back NFD for
 * composed characters. A raw prefix compare would call `café/photo.png`
 * outside a folder it is plainly inside, and import a duplicate of it.
 */
export function relativeInside(abs: string, root: string): string | null {
  if (!root) return null;
  const a = normalizePath(abs);
  const r = normalizePath(root.endsWith('/') ? root : `${root}/`);
  return a.startsWith(r) ? a.slice(r.length) : null;
}

/**
 * True for `.moss/` and anything under it — the site's inputs and machinery
 * rather than its content. Readers: the internal-toggle re-home in FileTree,
 * the preview follower's permanent-miss rule, and the editor's site-wide
 * scope note.
 */
export function isMossInternalPath(relPath: string): boolean {
  const norm = relPath.replace(/\\/g, '/').replace(/^\.\//, '');
  return norm === '.moss' || norm.startsWith('.moss/');
}
