/**
 * OS drag-in handler for the moss editor file tree.
 *
 * Handles files dragged from Finder/Explorer/Photos onto the editor panel.
 * Uses Tauri 2's native `onDragDropEvent` (NOT HTML5 dataTransfer.files —
 * mutually exclusive with the Tauri handler on Windows) which is already
 * registered in editor-main.ts. This module provides the handler logic that
 * editor-main.ts delegates to for file-tree drops.
 *
 * ## Architecture
 *
 * The existing `onDragDropEvent` in editor-main.ts handles both:
 * 1. CM editor drops (images) — handled by handleEditorFileDrop()
 * 2. File-tree drops — delegated here via `handleOsDragOver` / `handleOsDrop`
 *
 * This module does NOT register its own listener to avoid double-firing.
 * editor-main.ts calls the exported functions at the right points.
 *
 * ## Singleton vs factory
 *
 * Use `createOsDragInController(fileTree)` to create an instance-scoped
 * controller that closes over its own `activeHighlightId`, so multiple trees
 * and tests do not share state.
 *
 * The free-function exports (`handleOsDragOver`, `handleOsDrop`,
 * `clearOsDragHighlight`) delegate to a module-level instance created on
 * demand.
 *
 * ## Coordinate space
 *
 * Tauri 2 delivers physical-pixel coordinates in `onDragDropEvent`. Divide
 * by `window.devicePixelRatio` before calling getFolderAtPosition(), which
 * works in CSS (logical) pixels via getBoundingClientRect().
 */

import { host, type CopyInReport } from "../host";
import { ASSET_REGISTRY, lookupAsset } from './asset-registry.generated';
import type { MediaPickKind } from './media-pick-kind';
import { t } from "../i18n/index";
import { showToast } from "../shared/ui/toast/toast-manager";
import { toAbsolute, childPath, relativeInside, parentDirOf, baseName } from './path-domain';
import type { FileTree } from './tree';
import type { EntryId } from './entry-registry';
import { engine } from '../engine';

// ── Shared post-import follow-through ─────────────────────────────────────────

/**
 * Post-import follow-through shared by BOTH OS-drop paths:
 *
 *  - drop over the file tree → `handleOsDrop` (this module)
 *  - drop over the editor    → `handleEditorFileDrop` (editor-main.ts)
 *
 * Every path that lands a copy in the vault must call this so the new files
 * appear in the tree immediately. The two sibling flows drifted once — the
 * editor-drop path copied the file and inserted the embed but never refreshed,
 * leaving the import invisible in the tree until an unrelated refresh — so the
 * step lives in ONE named place that both handlers call.
 *
 * Note: the edit gate parks `render()` while an inline rename/create is open.
 * That's fine — refreshTree still updates the data model and the parked work
 * flushes when the edit finishes.
 */
export async function afterVaultImport(fileTree: FileTree | null): Promise<void> {
  await fileTree?.refreshAndRender();
}

// ── OsDragInController interface ──────────────────────────────────────────────

export interface OsDragInController {
  /**
   * Handle an OS drag-over event on the file tree area.
   *
   * Determines which folder (if any) is under the cursor and:
   * - updates the drop-target highlight
   * - arms spring-load for collapsed folders
   *
   * @param logicalX CSS-pixel X coordinate (physical / devicePixelRatio).
   * @param logicalY CSS-pixel Y coordinate (physical / devicePixelRatio).
   */
  handleOsDragOver(logicalX: number, logicalY: number): void;

  /**
   * Handle an OS drop event on the file tree area.
   *
   * Determines the target directory (folder under cursor, or project root as
   * fallback) and calls `copyFilesToProject` to copy the OS files in.
   * Refreshes the tree after import.
   *
   * @param paths    OS file paths from `event.payload.paths`.
   * @param logicalX CSS-pixel X coordinate (physical / devicePixelRatio).
   * @param logicalY CSS-pixel Y coordinate (physical / devicePixelRatio).
   */
  handleOsDrop(paths: string[], logicalX: number, logicalY: number): Promise<void>;

  /**
   * Handle an OS drag-leave or drag-cancel event on the file tree area.
   * Clears all drop highlights and cancels any pending spring-load.
   */
  clearOsDragHighlight(): void;
}

// ── Factory ───────────────────────────────────────────────────────────────────

/**
 * Create a per-tree OS drag-in controller.
 *
 * The controller holds no state of its own. It used to memoize which folder it
 * had last highlighted, which was a THIRD answer to "what is highlighted"
 * beside the tree's own — and it could disagree: anything that cleared the
 * tree's feedback (a project switch mid-drag, a drop the tree handled) wiped
 * the class while the memo still named the folder, so the next hover over that
 * same folder skipped the repaint and the highlight stayed gone. `highlight()`
 * is a clear-then-set over the whole surface, so asking for it again is free
 * and the memo bought nothing.
 *
 * @param fileTree The mounted FileTree instance to operate on.
 */
export function createOsDragInController(fileTree: FileTree): OsDragInController {
  function handleOsDragOver(logicalX: number, logicalY: number): void {
    const folderId = fileTree.getFolderAtPosition(logicalX, logicalY);
    fileTree.setDropHighlight(folderId);

    // Spring-load: arm timer for the hovered folder, or cancel if over empty area
    if (folderId !== null) {
      const path = fileTree.pathOf(folderId);
      fileTree.springLoadIfHoverIs(path);
    } else {
      fileTree.springLoadIfHoverIs(null);
    }
  }

  async function handleOsDrop(paths: string[], logicalX: number, logicalY: number): Promise<void> {
    clearOsDragHighlight();

    if (paths.length === 0) return;

    // Determine target: folder under cursor, or project root as fallback.
    //
    // `pathOf` returns a project-root-RELATIVE path (per entry-registry.ts), but the command
    // resolves its target against std::path (the process CWD, not the vault),
    // so it needs an ABSOLUTE path. Convert at the FS boundary — mirroring the
    // internal tree-drop move (see file-tree.ts performDrop → moveEntries).
    //
    // This OS drag-in was the lone call site the relative-path migration
    // missed: a folder drop passed the bare relative name (e.g. "Writings"),
    // which Rust's `PathBuf::from(target_dir).is_dir()` resolved against the
    // CWD → false → "Target is not a directory: Writings". `toAbsolute`
    // normalizes every case — folder → its dir, file-row → parent dir,
    // root-level (rel === '') → the project root — while leaving the
    // empty-space fallback (projectRootPath, already absolute) unchanged.
    const folderId = fileTree.getFolderAtPosition(logicalX, logicalY);
    const rel = folderId !== null ? fileTree.pathOf(folderId) : null;
    const targetDir = rel !== null
      ? toAbsolute(rel, fileTree.projectRootPath)
      : fileTree.projectRootPath;

    if (!targetDir || !host.copyIn) {
      // No project open, or a host with no files on disk to copy from
      return;
    }

    // The SAME command the editor drop uses. A separate tree-drop copy abandoned the
    // whole gesture on the first failure and reported `Err` for files that had
    // already landed, so an evicted iCloud photo took the rest of the drop with
    // it. `CopyReport` names each skip and why.
    const result = await host.copyIn(paths, targetDir);
    if (result.status !== 'ok') {
      console.error('OS drag-in: copyFilesToProject failed:', result.error);
      showToast({ what: t('tree.import_failed', { error: result.error }), severity: 'Blocking' });
      return;
    }
    reportImportSkips(result.data.skipped);

    // Refresh so the newly imported files appear in the tree immediately
    // (shared with the editor-drop path — see afterVaultImport's doc).
    await afterVaultImport(fileTree);
  }

  function clearOsDragHighlight(): void {
    fileTree.setDropHighlight(null);
    fileTree.springLoadIfHoverIs(null);
  }

  return { handleOsDragOver, handleOsDrop, clearOsDragHighlight };
}

// ── Legacy free-function API (backward-compat) ────────────────────────────────
//
// These three functions match the original module-level exports. Call sites in
// editor-main.ts pass `fileTree` at every call; we forward to a transient
// single-use controller so each invocation is stateless from the caller's
// perspective. This avoids a breaking change to editor-main.ts imports while
// keeping the module free of shared mutable state.
//
// For new code, prefer createOsDragInController(fileTree) so the controller
// is reused across over/drop/leave events in a single drag session, rather
// than allocating a new closure per event.

/**
 * Handle an OS drag-over event on the file tree area.
 *
 * @deprecated Prefer `createOsDragInController`. This legacy wrapper creates
 * a stateless per-call controller; highlight idempotency is lost across calls.
 *
 * @param fileTree The mounted FileTree instance.
 * @param logicalX CSS-pixel X coordinate (physical / devicePixelRatio).
 * @param logicalY CSS-pixel Y coordinate (physical / devicePixelRatio).
 */
export function handleOsDragOver(fileTree: FileTree, logicalX: number, logicalY: number): void {
  createOsDragInController(fileTree).handleOsDragOver(logicalX, logicalY);
}

/**
 * Handle an OS drop event on the file tree area.
 *
 * @deprecated Prefer `createOsDragInController`.
 *
 * @param fileTree The mounted FileTree instance.
 * @param paths    OS file paths from `event.payload.paths`.
 * @param logicalX CSS-pixel X coordinate (physical / devicePixelRatio).
 * @param logicalY CSS-pixel Y coordinate (physical / devicePixelRatio).
 */
export async function handleOsDrop(
  fileTree: FileTree,
  paths: string[],
  logicalX: number,
  logicalY: number,
): Promise<void> {
  await createOsDragInController(fileTree).handleOsDrop(paths, logicalX, logicalY);
}

/**
 * Handle an OS drag-leave or drag-cancel event on the file tree area.
 *
 * @deprecated Prefer `createOsDragInController`.
 *
 * @param fileTree The mounted FileTree instance.
 */
export function clearOsDragHighlight(fileTree: FileTree): void {
  createOsDragInController(fileTree).clearOsDragHighlight();
}

/**
 * Where a dropped/pasted/picked IMAGE for `pageRel` should be stored, as a
 * project-relative directory ('' = project root). Asks the backend, which
 * applies `[editor].attachment_folder` (next to the page / sibling subfolder /
 * root-relative folder);
 * the copy creates the folder. Falls back to the page's own directory (the
 * pre-setting behavior) if the command fails, so an IPC hiccup can't lose an
 * image. The inserted `![](path)` is written from the page to wherever the
 * file lands.
 */
export async function attachmentDirFor(pageRel: string): Promise<string> {
  try {
    const r = await engine.resolveAttachmentDir(pageRel);
    if (r.status === 'ok') return r.data;
  } catch {
    // fall through to the page-dir default
  }
  return pageRel.includes('/') ? pageRel.substring(0, pageRel.lastIndexOf('/')) : '';
}

/**
 * Tell the user about the files an into-vault copy did NOT land.
 *
 * The backend reports skips explicitly (`CopyReport.skipped`) because the
 * gesture used to succeed silently: a copy that failed for every file still
 * came back `Ok` with an empty list, and every caller here toasted only on
 * `status !== 'ok'`. Nothing appeared and nothing was said.
 *
 * The two reasons need different words, so they get different toasts. A cloud
 * placeholder is not a failure the user can fix by trying something else —
 * moss has asked the provider for the bytes, so the honest instruction is
 * "try again once it arrives". Anything else is a real failure.
 */
export function reportImportSkips(skipped: CopyInReport['skipped']): void {
  const waiting = skipped.filter((s) => s.reason === 'still_in_cloud');
  const failed = skipped.filter((s) => s.reason !== 'still_in_cloud');
  if (waiting.length > 0) {
    showToast({
      what: waiting.length === 1
        ? t('dnd.import_still_downloading_one', { name: waiting[0].original_name })
        : t('dnd.import_still_downloading_many', { n: waiting.length }),
      severity: 'NeedsAction',
    });
  }
  if (failed.length > 0) {
    showToast({
      what: failed.length === 1
        ? t('dnd.import_skipped_one', { name: failed[0].original_name })
        : t('dnd.import_skipped_many', { n: failed.length }),
      severity: 'Blocking',
    });
  }
}

/**
 * Where ONE file lands, by the same rule `importIntoVault` applies to a batch:
 * images to the attachment folder, everything else beside the page. The browser
 * upload asks per file because its dialog uploads as it picks.
 */
export async function importDirFor(fileName: string, pageRel: string): Promise<string> {
  if (lookupAsset(fileName)?.kind === 'Image') return attachmentDirFor(pageRel);
  return pageRel.includes('/') ? pageRel.substring(0, pageRel.lastIndexOf('/')) : '';
}

/**
 * Insert-bar media picker. Media/Gallery/
 * Hero open the system file dialog and import the picked files exactly like
 * an OS drop: copyFilesToProject returns collision-safe final names, which
 * this returns as project-relative paths for the bar's embeds. Lives here with
 * afterVaultImport because both are vault-import glue editor-main delegates.
 */
/**
 * Land files in the vault the way an OS drop would, and say where each one
 * went.
 *
 * ONE policy, three callers (the insert bar's media picker, the Browse button,
 * and an OS drop through the controller): images honour the
 * `[editor].attachment_folder` setting, everything else lands next to the page
 * — the setting is deliberately image-scoped. Splitting that decision per entry
 * point is how the two already began to drift.
 *
 * Returns project-relative `{ dir, finalName }` per imported file — the final
 * name is the backend's collision-safe one, which is why this cannot just echo
 * what it was given. `null` means the import failed and has already been
 * reported to the user.
 */
export async function importIntoVault(
  paths: string[],
  pageRel: string,
  projectRoot: string,
): Promise<Array<{ dir: string; finalName: string }> | null> {
  const pageDir = pageRel.includes('/') ? pageRel.substring(0, pageRel.lastIndexOf('/')) : '';
  const images = paths.filter((p) => lookupAsset(p)?.kind === 'Image');
  const others = paths.filter((p) => lookupAsset(p)?.kind !== 'Image');
  const copyIn = host.copyIn;
  // A host that cannot read outside the folder has already landed its picks inside it.
  if (!copyIn) return null;
  const landed: Array<{ dir: string; finalName: string }> = [];
  for (const [batch, targetRel] of [
    [images, images.length > 0 ? await attachmentDirFor(pageRel) : pageDir],
    [others, pageDir],
  ] as Array<[string[], string]>) {
    if (batch.length === 0) continue;
    const r = await copyIn(batch, toAbsolute(targetRel, projectRoot));
    if (r.status !== 'ok') {
      console.error('vault import failed', r.error);
      showToast({ what: t('dnd.import_files_failed'), severity: 'Blocking' });
      return null;
    }
    reportImportSkips(r.data.skipped);
    landed.push(...r.data.copied.map((f) => ({ dir: targetRel, finalName: f.final_name })));
  }
  // Asked for files and landed none: the gesture failed, and the skips have
  // already said why. Returning `[]` here would let callers treat a total
  // failure as an import of zero files — the silence this whole path exists
  // to end.
  if (paths.length > 0 && landed.length === 0) return null;
  return landed;
}

export async function pickMediaForBar(
  kind: MediaPickKind,
  ctx: { currentFilePath(): string | null; projectRoot(): string | null },
): Promise<string[] | null> {
  // Hero backgrounds accept images AND videos (video slides render as
  // ambient loops); gallery is an image set; media embeds anything.
  const kinds =
    kind === 'media' ? ['Image', 'Video', 'Audio']
    : kind === 'hero' ? ['Image', 'Video']
    : ['Image'];
  const extensions = ASSET_REGISTRY
    .filter((a) => kinds.includes(a.kind) && a.canEmbed)
    .map((a) => a.ext);
  // The host's dialog answers `[]` where it has none, which every caller here
  // treats as a cancel. Gallery and hero are multi-media blocks (hero:
  // crossfading slides).
  const path = ctx.currentFilePath();
  if (!path) return null;
  const paths = await host.pickFiles(
    [{ name: kind === 'gallery' ? 'Images' : 'Media', extensions }],
    kind === 'gallery' || kind === 'hero',
    (name) => importDirFor(name, path),
  );
  if (paths.length === 0) return null;

  // A file already inside the folder is embedded where it is: a host that
  // uploads the pick has already landed it there, and copying it again would
  // mint a `-2` duplicate.
  const root = ctx.projectRoot() ?? '';
  const inside: Array<{ dir: string; finalName: string }> = [];
  const outside: string[] = [];
  for (const p of paths) {
    const rel = relativeInside(p, root);
    if (rel === null) outside.push(p);
    else inside.push({ dir: parentDirOf(rel), finalName: baseName(rel) });
  }
  const imported = outside.length > 0 ? await importIntoVault(outside, path, root) : [];
  const landed = imported && [...inside, ...imported];
  // Project-relative: a Media embed is written as the path from the page.
  return landed?.map((f) => childPath(f.dir, f.finalName)) ?? null;
}
