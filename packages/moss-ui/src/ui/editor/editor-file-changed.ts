// Dependency-injected dispatcher for FileChanged events.
//
// This is the function that editor-main.ts's single onMossEvent('FileChanged')
// handler (registered via EditorFileEvents) delegates to. It merges what were
// previously two independent inline listeners in editor-main.ts:
//   - registry sync (rename → upsert → delete), always unconditional
//   - onboarding/import-refresh (no file open, new files arrived)
//   - body/frontmatter/heading reload (file open, dirty-aware Zed-style defer)
//
// This module is pure logic: no Tauri imports, no DOM access. All side-effects
// are injected via EditorFileChangedDeps, keeping the function testable with
// vitest without mocking the Tauri bridge.

import type { FileChangeEvent } from '../preview/preview-actions';
import type { PendingRenames } from './session/pending-renames';

export interface EditorFileChangedDeps {
  // EntryRegistry — null if the registry hasn't been initialised yet.
  registry: {
    rename(oldPath: string, newPath: string): void;
    upsert(path: string, isDir: boolean): unknown;
    delete(path: string): void;
    getByPath(path: string): unknown;
  } | null;
  // Project root on disk (absolute path). Null before the folder is resolved.
  projectRoot: string | null;
  // Returns the project-root-relative path of the currently open file, or
  // null if no file is open (onboarding / folder-mode / empty-state).
  currentFilePath(): string | null;
  // Returns the EntryId of the currently open file, or null when unavailable.
  currentEntryId(): number | null;
  pendingRenames: PendingRenames;
  /** The mounted-document owner reconciles body, frontmatter, heading and mtimes. */
  reloadExternalDocument(): Promise<void>;
  // Flash file-tree rows for newly-created paths (download/import indicator).
  flashDownloadTargets(created: string[]): void;
  // Reconcile the selected folder from disk after any filesystem change while
  // no document is open, or after the open document was deleted. Directory-only
  // changes matter too: the final empty subfolder can be removed externally.
  onboardingRefresh(deletedSource?: string): Promise<void>;
  /** The open document's file was deleted (or moved away unpaired) outside
   *  moss. True when the editor keeps the document open because it holds
   *  unsaved text; false when it lets it close. */
  documentDeletedOutside(path: string): boolean;
  /** True while the editor holds a document whose file is gone. */
  holdsDetachedDocument(): boolean;
}

/**
 * Unified FileChanged dispatcher.
 *
 * The one dependency-injected, testable FileChanged dispatcher. The execution
 * order is:
 *
 * (a) Registry sync — ALWAYS, ordered rename → create → delete.
 *     Keeps EntryRegistry consistent even when the current file isn't affected.
 *     Source-domain paths from Rust's watch.rs are already project-root-relative;
 *     no toAbsolute conversion needed here.
 *
 * (b) No file open, or its source was deleted → folder reconciliation.
 *     Refresh the selected directory from disk, including directory-only
 *     removals, and return without asking the editor to reread a deleted file.
 *
 * (c) File open → dirty-aware body/frontmatter/heading reload.
 *     Pending-rename suppression guards the modified_paths and source_renames
 *     fields (source-domain only — moved_output_paths is output-domain and
 *     never matches stored source paths), and retires the record
 *     on the matching echo rather than on a clock.
 *     EntryId match (stable across rename) is the primary identity check;
 *     path inclusion is the fallback when currentEntryId() is null.
 *     The document owner handles dirty-state deferral and reconciles the whole
 *     snapshot; this dispatcher never captures an editor or store across IPC.
 */
export async function dispatchEditorFileChanged(
  fce: FileChangeEvent,
  d: EditorFileChangedDeps,
): Promise<void> {
  const previousPath = d.currentFilePath();
  // (a) Registry sync — unconditional, rename → create → delete.
  // Source-domain wire paths from Rust's watch.rs:path_to_relative_key are
  // ALREADY project-root-relative, and EntryRegistry stores relative paths
  // internally too, so they pass straight through with no conversion.
  if (fce && d.registry && d.projectRoot !== null) {
    for (const [oldPath, newPath] of fce.source_renames ?? []) {
      d.registry.rename(oldPath, newPath);
    }
    for (const newPath of fce.source_creates ?? []) {
      d.registry.upsert(newPath, /* isDir */ false);
    }
    for (const deletedPath of fce.source_deletes ?? []) {
      d.registry.delete(deletedPath);
    }
    // Identity preservation: not handled here. registry.rename above already
    // remapped byPath such that currentEntryId's record (if it was the rename's
    // old side) now resolves to the new path via currentFilePath() — no explicit
    // reassignment needed.
  }

  // Download flash — fires on EVERY new-file arrival, independent of whether a
  // file is open (matches the original onboarding listener: flash precedes the
  // currentFilePath() early-return). Viewport-only; never expands the tree.
  const created = fce.source_creates ?? [];
  if (created.length > 0) d.flashDownloadTargets(created);

  const path = d.currentFilePath();

  // (b) No file open → onboarding/import case.
  // A file already open owns the surface — the file-tree's own FileChanged
  // listener (refreshAndRender) refreshes the rows live, and re-deciding
  // the root surface here would stomp the active edit. So the surface
  // re-decision below is the ONBOARDING case only (no file open).
  const deletedOpen = previousPath !== null && (fce.source_deletes ?? []).includes(previousPath);
  // Unsaved text outlives its file: the editor keeps the document, and later
  // events leave it alone until the author saves it back or discards it.
  if (deletedOpen && d.documentDeletedOutside(previousPath)) return;
  if (!path && !deletedOpen && d.holdsDetachedDocument()) return;
  if (!path || (fce.source_deletes ?? []).includes(path)) {
    await d.onboardingRefresh(deletedOpen ? previousPath : undefined);
    return;
  }

  // (c) A file is open → dirty-aware body/frontmatter/heading reload.

  // The source files whose bytes changed this rebuild (another app's edit, or
  // our own save), project-root-relative like the registry's keys. This is the
  // one field that says the open source file changed: moved_output_paths is
  // output-domain and never names a source path.
  const modified = fce.modified_paths ?? [];
  // source_renames carries project-root-relative source paths — the same
  // domain as the registered renames (entry.path from the registry).
  const sourceRenamed = fce.source_renames ?? [];

  // Swallow the echo of a rename moss itself issued — the watcher fires for
  // both sides, and the editor's own write-then-rename adds a stale event for
  // the old path. The rename transaction calls the same external reload owner.
  // Probed against modified_paths and source_renames, both
  // source-domain; moved_output_paths is output-domain and never matches the
  // stored source paths.
  if (d.pendingRenames.swallows(modified, sourceRenamed)) return;

  // Primary: EntryId match (stable across rename). Fallback to path inclusion
  // when currentEntryId is null (registry unavailable or no file loaded).
  const activeId = d.currentEntryId();
  const affectsCurrentFile = activeId !== null
    ? modified.some((p) => d.registry?.getByPath(p) === activeId)
    : modified.includes(path);
  if (!affectsCurrentFile) return;

  await d.reloadExternalDocument();
}
